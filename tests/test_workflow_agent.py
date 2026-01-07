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
                        {"id": "provider", "label": "provider", "type": "provider"},
                        {"id": "model", "label": "model", "type": "model"},
                        {"id": "tools", "label": "tools", "type": "tools"},
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

def _make_agent_v1_flow_with_meta(*, flow_id: str, name: str) -> dict:
    """Flow that wires a JSON literal into On Flow End.meta."""
    base = _make_agent_v1_flow_dict(flow_id=flow_id, name=name, declare_interface=True)
    base_nodes = list(base.get("nodes") or [])
    # Add meta pin on end and a JSON literal node.
    for n in base_nodes:
        if n.get("id") == "end" and isinstance(n.get("data"), dict):
            pins = list(n["data"].get("inputs") or [])
            pins.append({"id": "meta", "label": "meta", "type": "object"})
            n["data"]["inputs"] = pins
    base_nodes.append(
        {
            "id": "meta_lit",
            "type": "literal_json",
            "position": {"x": 5, "y": -10},
            "data": {"literalValue": {"foo": "bar"}},
        }
    )
    base["nodes"] = base_nodes
    base_edges = list(base.get("edges") or [])
    base_edges.append(
        {
            "id": "edge-meta",
            "source": "meta_lit",
            "sourceHandle": "value",
            "target": "end",
            "targetHandle": "meta",
            "animated": False,
        }
    )
    base["edges"] = base_edges
    return base


def _make_agent_v1_flow_with_status_event(*, flow_id: str, name: str) -> dict:
    """Flow that emits `abstractcode.status` via Emit Event and then returns response."""
    base = _make_agent_v1_flow_dict(flow_id=flow_id, name=name, declare_interface=True)
    # Insert an emit_event node between start and end.
    base["nodes"].append(
        {
            "id": "emit",
            "type": "emit_event",
            "position": {"x": 5, "y": 10},
            "data": {
                "inputs": [
                    {"id": "exec-in", "label": "", "type": "execution"},
                    {"id": "name", "label": "name", "type": "string"},
                    {"id": "payload", "label": "payload", "type": "any"},
                ],
                "outputs": [{"id": "exec-out", "label": "", "type": "execution"}],
            },
        }
    )
    base["nodes"].append(
        {
            "id": "status_name",
            "type": "literal_string",
            "position": {"x": 2, "y": 20},
            "data": {"literalValue": "abstractcode.status"},
        }
    )
    base["nodes"].append(
        {
            "id": "status_payload",
            "type": "literal_string",
            "position": {"x": 2, "y": 30},
            "data": {"literalValue": "Enrich Query..."},
        }
    )

    # Rewrite exec edge: start -> emit -> end
    edges = [e for e in base.get("edges") or [] if e.get("id") != "edge-exec"]
    edges.append(
        {
            "id": "edge-exec-1",
            "source": "start",
            "sourceHandle": "exec-out",
            "target": "emit",
            "targetHandle": "exec-in",
            "animated": True,
        }
    )
    edges.append(
        {
            "id": "edge-exec-2",
            "source": "emit",
            "sourceHandle": "exec-out",
            "target": "end",
            "targetHandle": "exec-in",
            "animated": True,
        }
    )
    # Wire name/payload into emit
    edges.append(
        {
            "id": "edge-status-name",
            "source": "status_name",
            "sourceHandle": "value",
            "target": "emit",
            "targetHandle": "name",
            "animated": False,
        }
    )
    edges.append(
        {
            "id": "edge-status-payload",
            "source": "status_payload",
            "sourceHandle": "value",
            "target": "emit",
            "targetHandle": "payload",
            "animated": False,
        }
    )
    base["edges"] = edges
    return base


def _make_agent_v1_flow_with_status_event_duration(*, flow_id: str, name: str, duration_s: float) -> dict:
    """Flow that emits `abstractcode.status` with a duration payload."""
    base = _make_agent_v1_flow_dict(flow_id=flow_id, name=name, declare_interface=True)
    base["nodes"].append(
        {
            "id": "emit",
            "type": "emit_event",
            "position": {"x": 5, "y": 10},
            "data": {
                "inputs": [
                    {"id": "exec-in", "label": "", "type": "execution"},
                    {"id": "name", "label": "name", "type": "string"},
                    {"id": "payload", "label": "payload", "type": "any"},
                ],
                "outputs": [{"id": "exec-out", "label": "", "type": "execution"}],
            },
        }
    )
    base["nodes"].append(
        {
            "id": "status_name",
            "type": "literal_string",
            "position": {"x": 2, "y": 20},
            "data": {"literalValue": "abstractcode.status"},
        }
    )
    base["nodes"].append(
        {
            "id": "status_payload",
            "type": "literal_json",
            "position": {"x": 2, "y": 30},
            "data": {"literalValue": {"text": "Enrich Query...", "duration": duration_s}},
        }
    )

    edges = [e for e in base.get("edges") or [] if e.get("id") != "edge-exec"]
    edges.append(
        {
            "id": "edge-exec-1",
            "source": "start",
            "sourceHandle": "exec-out",
            "target": "emit",
            "targetHandle": "exec-in",
            "animated": True,
        }
    )
    edges.append(
        {
            "id": "edge-exec-2",
            "source": "emit",
            "sourceHandle": "exec-out",
            "target": "end",
            "targetHandle": "exec-in",
            "animated": True,
        }
    )
    edges.append(
        {
            "id": "edge-status-name",
            "source": "status_name",
            "sourceHandle": "value",
            "target": "emit",
            "targetHandle": "name",
            "animated": False,
        }
    )
    edges.append(
        {
            "id": "edge-status-payload",
            "source": "status_payload",
            "sourceHandle": "value",
            "target": "emit",
            "targetHandle": "payload",
            "animated": False,
        }
    )
    base["edges"] = edges
    return base


def _make_agent_v1_flow_with_message_event(*, flow_id: str, name: str) -> dict:
    """Flow that emits `abstractcode.message` via Emit Event and then returns response."""
    base = _make_agent_v1_flow_dict(flow_id=flow_id, name=name, declare_interface=True)
    base["nodes"].append(
        {
            "id": "emit",
            "type": "emit_event",
            "position": {"x": 5, "y": 10},
            "data": {
                "inputs": [
                    {"id": "exec-in", "label": "", "type": "execution"},
                    {"id": "name", "label": "name", "type": "string"},
                    {"id": "payload", "label": "payload", "type": "any"},
                ],
                "outputs": [{"id": "exec-out", "label": "", "type": "execution"}],
            },
        }
    )
    base["nodes"].append(
        {
            "id": "msg_name",
            "type": "literal_string",
            "position": {"x": 2, "y": 20},
            "data": {"literalValue": "abstractcode.message"},
        }
    )
    base["nodes"].append(
        {
            "id": "msg_payload",
            "type": "literal_string",
            "position": {"x": 2, "y": 30},
            "data": {"literalValue": "Hello from workflow"},
        }
    )

    edges = [e for e in base.get("edges") or [] if e.get("id") != "edge-exec"]
    edges.append(
        {
            "id": "edge-exec-1",
            "source": "start",
            "sourceHandle": "exec-out",
            "target": "emit",
            "targetHandle": "exec-in",
            "animated": True,
        }
    )
    edges.append(
        {
            "id": "edge-exec-2",
            "source": "emit",
            "sourceHandle": "exec-out",
            "target": "end",
            "targetHandle": "exec-in",
            "animated": True,
        }
    )
    edges.append(
        {
            "id": "edge-msg-name",
            "source": "msg_name",
            "sourceHandle": "value",
            "target": "emit",
            "targetHandle": "name",
            "animated": False,
        }
    )
    edges.append(
        {
            "id": "edge-msg-payload",
            "source": "msg_payload",
            "sourceHandle": "value",
            "target": "emit",
            "targetHandle": "payload",
            "animated": False,
        }
    )
    base["edges"] = edges
    return base


def _make_agent_v1_flow_with_tool_events(*, flow_id: str, name: str) -> dict:
    """Flow that emits tool execution/result via Emit Event for AbstractCode UX."""
    base = _make_agent_v1_flow_dict(flow_id=flow_id, name=name, declare_interface=True)
    base["nodes"].append(
        {
            "id": "emit_exec",
            "type": "emit_event",
            "position": {"x": 5, "y": 10},
            "data": {
                "inputs": [
                    {"id": "exec-in", "label": "", "type": "execution"},
                    {"id": "name", "label": "name", "type": "string"},
                    {"id": "payload", "label": "payload", "type": "any"},
                ],
                "outputs": [{"id": "exec-out", "label": "", "type": "execution"}],
            },
        }
    )
    base["nodes"].append(
        {
            "id": "emit_result",
            "type": "emit_event",
            "position": {"x": 9, "y": 10},
            "data": {
                "inputs": [
                    {"id": "exec-in", "label": "", "type": "execution"},
                    {"id": "name", "label": "name", "type": "string"},
                    {"id": "payload", "label": "payload", "type": "any"},
                ],
                "outputs": [{"id": "exec-out", "label": "", "type": "execution"}],
            },
        }
    )
    base["nodes"].append(
        {
            "id": "name_exec",
            "type": "literal_string",
            "position": {"x": 2, "y": 20},
            "data": {"literalValue": "abstractcode.tool_execution"},
        }
    )
    base["nodes"].append(
        {
            "id": "name_result",
            "type": "literal_string",
            "position": {"x": 2, "y": 30},
            "data": {"literalValue": "abstractcode.tool_result"},
        }
    )
    base["nodes"].append(
        {
            "id": "payload_exec",
            "type": "literal_json",
            "position": {"x": 2, "y": 40},
            "data": {
                "literalValue": {
                    "tool": "read_file",
                    "call_id": "c1",
                    "arguments": {"target_file": "docs/architecture.md"},
                }
            },
        }
    )
    base["nodes"].append(
        {
            "id": "payload_result",
            "type": "literal_json",
            "position": {"x": 2, "y": 50},
            "data": {
                "literalValue": {
                    "tool": "read_file",
                    "call_id": "c1",
                    "success": True,
                    "output": "ok",
                }
            },
        }
    )

    edges = [e for e in base.get("edges") or [] if e.get("id") != "edge-exec"]
    edges.append(
        {
            "id": "edge-exec-1",
            "source": "start",
            "sourceHandle": "exec-out",
            "target": "emit_exec",
            "targetHandle": "exec-in",
            "animated": True,
        }
    )
    edges.append(
        {
            "id": "edge-exec-2",
            "source": "emit_exec",
            "sourceHandle": "exec-out",
            "target": "emit_result",
            "targetHandle": "exec-in",
            "animated": True,
        }
    )
    edges.append(
        {
            "id": "edge-exec-3",
            "source": "emit_result",
            "sourceHandle": "exec-out",
            "target": "end",
            "targetHandle": "exec-in",
            "animated": True,
        }
    )
    edges.append(
        {
            "id": "edge-name-exec",
            "source": "name_exec",
            "sourceHandle": "value",
            "target": "emit_exec",
            "targetHandle": "name",
            "animated": False,
        }
    )
    edges.append(
        {
            "id": "edge-name-result",
            "source": "name_result",
            "sourceHandle": "value",
            "target": "emit_result",
            "targetHandle": "name",
            "animated": False,
        }
    )
    edges.append(
        {
            "id": "edge-payload-exec",
            "source": "payload_exec",
            "sourceHandle": "value",
            "target": "emit_exec",
            "targetHandle": "payload",
            "animated": False,
        }
    )
    edges.append(
        {
            "id": "edge-payload-result",
            "source": "payload_result",
            "sourceHandle": "value",
            "target": "emit_result",
            "targetHandle": "payload",
            "animated": False,
        }
    )
    base["edges"] = edges
    return base


def _make_agent_v1_flow_with_tool_events_batch(*, flow_id: str, name: str) -> dict:
    """Flow that emits *batched* tool execution/result payloads (lists) for wiring convenience."""
    base = _make_agent_v1_flow_dict(flow_id=flow_id, name=name, declare_interface=True)
    base["nodes"].append(
        {
            "id": "emit_exec",
            "type": "emit_event",
            "position": {"x": 5, "y": 10},
            "data": {
                "inputs": [
                    {"id": "exec-in", "label": "", "type": "execution"},
                    {"id": "name", "label": "name", "type": "string"},
                    {"id": "payload", "label": "payload", "type": "any"},
                ],
                "outputs": [{"id": "exec-out", "label": "", "type": "execution"}],
            },
        }
    )
    base["nodes"].append(
        {
            "id": "emit_result",
            "type": "emit_event",
            "position": {"x": 9, "y": 10},
            "data": {
                "inputs": [
                    {"id": "exec-in", "label": "", "type": "execution"},
                    {"id": "name", "label": "name", "type": "string"},
                    {"id": "payload", "label": "payload", "type": "any"},
                ],
                "outputs": [{"id": "exec-out", "label": "", "type": "execution"}],
            },
        }
    )
    base["nodes"].append(
        {
            "id": "name_exec",
            "type": "literal_string",
            "position": {"x": 2, "y": 20},
            "data": {"literalValue": "abstractcode.tool_execution"},
        }
    )
    base["nodes"].append(
        {
            "id": "name_result",
            "type": "literal_string",
            "position": {"x": 2, "y": 30},
            "data": {"literalValue": "abstractcode.tool_result"},
        }
    )
    base["nodes"].append(
        {
            "id": "payload_exec",
            "type": "literal_json",
            "position": {"x": 2, "y": 40},
            "data": {
                "literalValue": [
                    {
                        "name": "read_file",
                        "call_id": "c1",
                        "arguments": {"target_file": "docs/architecture.md"},
                    },
                    {
                        "id": "c2",
                        "type": "function",
                        "function": {"name": "read_file", "arguments": "{\"target_file\":\"docs/architecture.md\"}"},
                    },
                ]
            },
        }
    )
    base["nodes"].append(
        {
            "id": "payload_result",
            "type": "literal_json",
            "position": {"x": 2, "y": 50},
            "data": {
                "literalValue": [
                    {"tool": "read_file", "call_id": "c1", "success": True, "output": "ok"},
                    {"tool": "read_file", "call_id": "c2", "success": True, "output": "ok2"},
                ]
            },
        }
    )

    edges = [e for e in base.get("edges") or [] if e.get("id") != "edge-exec"]
    edges.append(
        {
            "id": "edge-exec-1",
            "source": "start",
            "sourceHandle": "exec-out",
            "target": "emit_exec",
            "targetHandle": "exec-in",
            "animated": True,
        }
    )
    edges.append(
        {
            "id": "edge-exec-2",
            "source": "emit_exec",
            "sourceHandle": "exec-out",
            "target": "emit_result",
            "targetHandle": "exec-in",
            "animated": True,
        }
    )
    edges.append(
        {
            "id": "edge-exec-3",
            "source": "emit_result",
            "sourceHandle": "exec-out",
            "target": "end",
            "targetHandle": "exec-in",
            "animated": True,
        }
    )
    edges.append(
        {
            "id": "edge-name-exec",
            "source": "name_exec",
            "sourceHandle": "value",
            "target": "emit_exec",
            "targetHandle": "name",
            "animated": False,
        }
    )
    edges.append(
        {
            "id": "edge-name-result",
            "source": "name_result",
            "sourceHandle": "value",
            "target": "emit_result",
            "targetHandle": "name",
            "animated": False,
        }
    )
    edges.append(
        {
            "id": "edge-payload-exec",
            "source": "payload_exec",
            "sourceHandle": "value",
            "target": "emit_exec",
            "targetHandle": "payload",
            "animated": False,
        }
    )
    edges.append(
        {
            "id": "edge-payload-result",
            "source": "payload_result",
            "sourceHandle": "value",
            "target": "emit_result",
            "targetHandle": "payload",
            "animated": False,
        }
    )
    base["edges"] = edges
    return base


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


def test_workflow_agent_propagates_meta_pin_to_assistant_message(tmp_path) -> None:
    try:
        from abstractflow.visual.models import VisualFlow
    except Exception:
        pytest.skip("abstractflow not installed")

    from abstractruntime import InMemoryRunStore, Runtime
    from abstractruntime.core.models import RunStatus
    from abstractruntime.storage.in_memory import InMemoryLedgerStore
    from abstractruntime.storage.observable import ObservableLedgerStore

    from abstractcode.workflow_agent import WorkflowAgent

    vf = VisualFlow.model_validate(_make_agent_v1_flow_with_meta(flow_id="wf_meta", name="wf_meta"))
    flow_path = tmp_path / "wf.json"
    flow_path.write_text(vf.model_dump_json(indent=2), encoding="utf-8")

    runtime = Runtime(run_store=InMemoryRunStore(), ledger_store=ObservableLedgerStore(InMemoryLedgerStore()))
    agent = WorkflowAgent(runtime=runtime, flow_ref=str(flow_path), tools=[])

    agent.start("hello")
    state = agent.step()
    while state.status == RunStatus.RUNNING:
        state = agent.step()

    assert state.status == RunStatus.COMPLETED
    ctx = state.vars.get("context") if isinstance(state.vars, dict) else None
    assert isinstance(ctx, dict)
    messages = ctx.get("messages")
    assert isinstance(messages, list)
    meta = (messages[-1].get("metadata") or {}) if isinstance(messages[-1], dict) else {}
    assert isinstance(meta, dict)
    assert meta.get("workflow_meta") == {"foo": "bar"}


def test_workflow_agent_emits_status_updates_from_emit_event(tmp_path) -> None:
    try:
        from abstractflow.visual.models import VisualFlow
    except Exception:
        pytest.skip("abstractflow not installed")

    from abstractruntime import InMemoryRunStore, Runtime
    from abstractruntime.core.models import RunStatus
    from abstractruntime.storage.in_memory import InMemoryLedgerStore
    from abstractruntime.storage.observable import ObservableLedgerStore

    from abstractcode.workflow_agent import WorkflowAgent

    vf = VisualFlow.model_validate(_make_agent_v1_flow_with_status_event(flow_id="wf_status", name="wf_status"))
    flow_path = tmp_path / "wf.json"
    flow_path.write_text(vf.model_dump_json(indent=2), encoding="utf-8")

    seen: list[dict] = []

    def on_step(step: str, data: dict) -> None:
        if step == "status":
            seen.append(dict(data))

    runtime = Runtime(run_store=InMemoryRunStore(), ledger_store=ObservableLedgerStore(InMemoryLedgerStore()))
    agent = WorkflowAgent(runtime=runtime, flow_ref=str(flow_path), tools=[], on_step=on_step)

    agent.start("hello")
    state = agent.step()
    while state.status == RunStatus.RUNNING:
        state = agent.step()

    assert state.status == RunStatus.COMPLETED
    assert any(s.get("text") == "Enrich Query..." for s in seen)


def test_workflow_agent_emits_status_duration_from_emit_event(tmp_path) -> None:
    try:
        from abstractflow.visual.models import VisualFlow
    except Exception:
        pytest.skip("abstractflow not installed")

    from abstractruntime import InMemoryRunStore, Runtime
    from abstractruntime.core.models import RunStatus
    from abstractruntime.storage.in_memory import InMemoryLedgerStore
    from abstractruntime.storage.observable import ObservableLedgerStore

    from abstractcode.workflow_agent import WorkflowAgent

    vf = VisualFlow.model_validate(
        _make_agent_v1_flow_with_status_event_duration(flow_id="wf_status_dur", name="wf_status_dur", duration_s=2.0)
    )
    flow_path = tmp_path / "wf.json"
    flow_path.write_text(vf.model_dump_json(indent=2), encoding="utf-8")

    seen: list[dict] = []

    def on_step(step: str, data: dict) -> None:
        if step == "status":
            seen.append(dict(data))

    runtime = Runtime(run_store=InMemoryRunStore(), ledger_store=ObservableLedgerStore(InMemoryLedgerStore()))
    agent = WorkflowAgent(runtime=runtime, flow_ref=str(flow_path), tools=[], on_step=on_step)

    agent.start("hello")
    state = agent.step()
    while state.status == RunStatus.RUNNING:
        state = agent.step()

    assert state.status == RunStatus.COMPLETED
    assert any(s.get("text") == "Enrich Query..." and s.get("duration") == 2.0 for s in seen)


def test_workflow_agent_emits_message_updates_from_emit_event(tmp_path) -> None:
    try:
        from abstractflow.visual.models import VisualFlow
    except Exception:
        pytest.skip("abstractflow not installed")

    from abstractruntime import InMemoryRunStore, Runtime
    from abstractruntime.core.models import RunStatus
    from abstractruntime.storage.in_memory import InMemoryLedgerStore
    from abstractruntime.storage.observable import ObservableLedgerStore

    from abstractcode.workflow_agent import WorkflowAgent

    vf = VisualFlow.model_validate(_make_agent_v1_flow_with_message_event(flow_id="wf_msg", name="wf_msg"))
    flow_path = tmp_path / "wf.json"
    flow_path.write_text(vf.model_dump_json(indent=2), encoding="utf-8")

    seen: list[dict] = []

    def on_step(step: str, data: dict) -> None:
        if step == "message":
            seen.append(dict(data))

    runtime = Runtime(run_store=InMemoryRunStore(), ledger_store=ObservableLedgerStore(InMemoryLedgerStore()))
    agent = WorkflowAgent(runtime=runtime, flow_ref=str(flow_path), tools=[], on_step=on_step)

    agent.start("hello")
    state = agent.step()
    while state.status == RunStatus.RUNNING:
        state = agent.step()

    assert state.status == RunStatus.COMPLETED
    assert any(str(s.get("text") or "") == "Hello from workflow" for s in seen)


def test_workflow_agent_emits_tool_events_from_emit_event(tmp_path) -> None:
    try:
        from abstractflow.visual.models import VisualFlow
    except Exception:
        pytest.skip("abstractflow not installed")

    from abstractruntime import InMemoryRunStore, Runtime
    from abstractruntime.core.models import RunStatus
    from abstractruntime.storage.in_memory import InMemoryLedgerStore
    from abstractruntime.storage.observable import ObservableLedgerStore

    from abstractcode.workflow_agent import WorkflowAgent

    vf = VisualFlow.model_validate(_make_agent_v1_flow_with_tool_events(flow_id="wf_tools", name="wf_tools"))
    flow_path = tmp_path / "wf.json"
    flow_path.write_text(vf.model_dump_json(indent=2), encoding="utf-8")

    seen_steps: list[str] = []

    def on_step(step: str, data: dict) -> None:
        if step in ("act", "observe"):
            seen_steps.append(step)

    runtime = Runtime(run_store=InMemoryRunStore(), ledger_store=ObservableLedgerStore(InMemoryLedgerStore()))
    agent = WorkflowAgent(runtime=runtime, flow_ref=str(flow_path), tools=[], on_step=on_step)

    agent.start("hello")
    state = agent.step()
    while state.status == RunStatus.RUNNING:
        state = agent.step()

    assert state.status == RunStatus.COMPLETED
    assert "act" in seen_steps
    assert "observe" in seen_steps


def test_workflow_agent_emits_batched_tool_events_from_emit_event(tmp_path) -> None:
    try:
        from abstractflow.visual.models import VisualFlow
    except Exception:
        pytest.skip("abstractflow not installed")

    from abstractruntime import InMemoryRunStore, Runtime
    from abstractruntime.core.models import RunStatus
    from abstractruntime.storage.in_memory import InMemoryLedgerStore
    from abstractruntime.storage.observable import ObservableLedgerStore

    from abstractcode.workflow_agent import WorkflowAgent

    vf = VisualFlow.model_validate(_make_agent_v1_flow_with_tool_events_batch(flow_id="wf_tools_batch", name="wf_tools_batch"))
    flow_path = tmp_path / "wf.json"
    flow_path.write_text(vf.model_dump_json(indent=2), encoding="utf-8")

    seen_steps: list[str] = []

    def on_step(step: str, data: dict) -> None:
        if step in ("act", "observe"):
            seen_steps.append(step)

    runtime = Runtime(run_store=InMemoryRunStore(), ledger_store=ObservableLedgerStore(InMemoryLedgerStore()))
    agent = WorkflowAgent(runtime=runtime, flow_ref=str(flow_path), tools=[], on_step=on_step)

    agent.start("hello")
    state = agent.step()
    while state.status == RunStatus.RUNNING:
        state = agent.step()

    assert state.status == RunStatus.COMPLETED
    assert seen_steps.count("act") >= 2
    assert seen_steps.count("observe") >= 2

def test_workflow_agent_runs_with_file_run_store(tmp_path) -> None:
    """Regression test: file-backed persistence must not blow up on cyclic vars.

    Historically, the VisualFlow `on_flow_start` node returned the full `run.vars`
    dict (including internal `_temp`). Because the visual executor persists per-node
    outputs in `vars["_temp"]["node_outputs"]`, that created a self-referential cycle
    which exploded during JsonFileRunStore.save() (dataclasses.asdict recursion).
    """
    try:
        from abstractflow.visual.models import VisualFlow
    except Exception:
        pytest.skip("abstractflow not installed")

    import json

    from abstractruntime import InMemoryLedgerStore, Runtime
    from abstractruntime.core.models import RunStatus
    from abstractruntime.storage.json_files import JsonFileRunStore

    from abstractcode.workflow_agent import WorkflowAgent

    vf = VisualFlow.model_validate(_make_agent_v1_flow_dict(flow_id="wf_file", name="wf_file", declare_interface=True))
    flow_path = tmp_path / "wf.json"
    flow_path.write_text(vf.model_dump_json(indent=2), encoding="utf-8")

    runtime = Runtime(run_store=JsonFileRunStore(tmp_path), ledger_store=InMemoryLedgerStore())
    agent = WorkflowAgent(runtime=runtime, flow_ref=str(flow_path), tools=[])

    agent.start("hello")
    state = agent.step()
    while state.status == RunStatus.RUNNING:
        state = agent.step()

    assert state.status == RunStatus.COMPLETED
    assert isinstance(state.output, dict)
    assert isinstance(state.output.get("result"), dict)
    assert state.output["result"]["response"] == "hello"

    # Ensure the run was actually persisted as valid JSON.
    run_file = tmp_path / f"run_{state.run_id}.json"
    assert run_file.exists()
    persisted = json.loads(run_file.read_text(encoding="utf-8"))
    assert isinstance(persisted, dict)
    assert persisted.get("run_id") == state.run_id


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

