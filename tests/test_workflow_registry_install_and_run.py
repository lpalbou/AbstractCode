from __future__ import annotations

import json
import zipfile
from pathlib import Path

import pytest


def _make_root_flow() -> dict:
    return {
        "id": "root",
        "name": "bundle-agent",
        "interfaces": ["abstractcode.agent.v1"],
        "nodes": [
            {
                "id": "start",
                "type": "on_flow_start",
                "position": {"x": 0, "y": 0},
                "data": {
                    "outputs": [
                        {"id": "exec-out", "label": "", "type": "execution"},
                        {"id": "prompt", "label": "prompt", "type": "string"},
                        {"id": "provider", "label": "provider", "type": "provider"},
                        {"id": "model", "label": "model", "type": "model"},
                        {"id": "tools", "label": "tools", "type": "tools"},
                    ]
                },
            },
            {
                "id": "call",
                "type": "subflow",
                "position": {"x": 5, "y": 0},
                "data": {
                    "subflowId": "sub",
                    "inputs": [
                        {"id": "exec-in", "label": "", "type": "execution"},
                        {"id": "prompt", "label": "prompt", "type": "string"},
                    ],
                    "outputs": [
                        {"id": "exec-out", "label": "", "type": "execution"},
                        {"id": "response", "label": "response", "type": "string"},
                    ],
                },
            },
            {
                "id": "end",
                "type": "on_flow_end",
                "position": {"x": 10, "y": 0},
                "data": {"inputs": [{"id": "exec-in", "label": "", "type": "execution"}, {"id": "response", "label": "response", "type": "string"}]},
            },
        ],
        "edges": [
            {"id": "e1", "source": "start", "sourceHandle": "exec-out", "target": "call", "targetHandle": "exec-in", "animated": True},
            {"id": "e2", "source": "start", "sourceHandle": "prompt", "target": "call", "targetHandle": "prompt", "animated": False},
            {"id": "e3", "source": "call", "sourceHandle": "exec-out", "target": "end", "targetHandle": "exec-in", "animated": True},
            {"id": "e4", "source": "call", "sourceHandle": "response", "target": "end", "targetHandle": "response", "animated": False},
        ],
        "entryNode": "start",
    }


def _make_sub_flow() -> dict:
    return {
        "id": "sub",
        "name": "sub",
        "interfaces": [],
        "nodes": [
            {
                "id": "start",
                "type": "on_flow_start",
                "position": {"x": 0, "y": 0},
                "data": {"outputs": [{"id": "exec-out", "label": "", "type": "execution"}, {"id": "prompt", "label": "prompt", "type": "string"}]},
            },
            {
                "id": "end",
                "type": "on_flow_end",
                "position": {"x": 10, "y": 0},
                "data": {"inputs": [{"id": "exec-in", "label": "", "type": "execution"}, {"id": "response", "label": "response", "type": "string"}]},
            },
        ],
        "edges": [
            {"id": "e1", "source": "start", "sourceHandle": "exec-out", "target": "end", "targetHandle": "exec-in", "animated": True},
            {"id": "e2", "source": "start", "sourceHandle": "prompt", "target": "end", "targetHandle": "response", "animated": False},
        ],
        "entryNode": "start",
    }


def _write_bundle(path: Path) -> None:
    manifest = {
        "bundle_format_version": "1",
        "bundle_id": "bundle-agent",
        "bundle_version": "0.0.1",
        "created_at": "2026-01-24T00:00:00Z",
        "entrypoints": [
            {
                "flow_id": "root",
                "name": "bundle-agent",
                "description": "",
                "interfaces": ["abstractcode.agent.v1"],
            }
        ],
        "default_entrypoint": "root",
        "flows": {"root": "flows/root.json", "sub": "flows/sub.json"},
        "metadata": {"test": True},
    }
    root = _make_root_flow()
    sub = _make_sub_flow()

    path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(path, "w") as zf:
        zf.writestr("manifest.json", json.dumps(manifest, ensure_ascii=False, indent=2))
        zf.writestr("flows/root.json", json.dumps(root, ensure_ascii=False, indent=2))
        zf.writestr("flows/sub.json", json.dumps(sub, ensure_ascii=False, indent=2))


@pytest.mark.integration
def test_workflow_registry_install_and_run_bundle_with_subflow_restart(tmp_path, monkeypatch) -> None:
    from abstractcode.workflow_agent import WorkflowAgent
    from abstractruntime import Runtime
    from abstractruntime.core.models import RunStatus
    from abstractruntime.storage.json_files import JsonFileRunStore, JsonlLedgerStore
    from abstractruntime.workflow_bundle import WorkflowBundleRegistry

    registry_dir = tmp_path / "registry"
    runtime_dir = tmp_path / "runtime"
    src_bundle = tmp_path / "src.flow"
    _write_bundle(src_bundle)

    reg = WorkflowBundleRegistry(registry_dir)
    reg.install(src_bundle)

    monkeypatch.setenv("ABSTRACTFRAMEWORK_WORKFLOWS_DIR", str(registry_dir))

    def _run_once() -> str:
        runtime = Runtime(
            run_store=JsonFileRunStore(runtime_dir),
            ledger_store=JsonlLedgerStore(runtime_dir),
        )
        agent = WorkflowAgent(runtime=runtime, flow_ref="bundle-agent", tools=[])
        run_id = agent.start("hello")
        state = agent.step()
        while state.status == RunStatus.RUNNING:
            state = agent.step()
        assert state.status == RunStatus.COMPLETED
        assert isinstance(state.output, dict)
        assert state.output.get("response") == "hello"
        assert state.workflow_id == "bundle-agent@0.0.1:root"
        return run_id

    run_id1 = _run_once()
    assert (runtime_dir / f"run_{run_id1}.json").exists()

    run_id2 = _run_once()
    assert run_id2 != run_id1

