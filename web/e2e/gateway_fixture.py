#!/usr/bin/env python3
"""Disposable real AbstractGateway fixture for AbstractCode browser checks.

This starts the production ASGI app on loopback with a *new* temporary data
root, a test-only authenticated user, and deterministic VisualFlow fixtures. It
never proxies or stubs a gateway route and deliberately configures no model
provider, so opening the fixture cannot reach an operator stack or invoke an
LLM.

Run from the monorepo root:

    .venv/bin/python abstractcode/web/e2e/gateway_fixture.py

The process owns its temporary directory for its lifetime. Ctrl-C stops the
server; no files or listeners survive the process.
"""

from __future__ import annotations

import argparse
import copy
import importlib.util
import io
import json
import os
import shutil
import signal
import sys
import tempfile
import zipfile
from pathlib import Path
from typing import Any


HOST = "127.0.0.1"
DEFAULT_PORT = 18781
USER_ID = "web-tester"
TOKEN = "abstractcode-e2e-only"  # Test fixture only; never accepted by a real gateway.
BUNDLE_ID = "abstractcode-web-e2e"
BUNDLE_VERSION = "0.0.1"


def _clear_inherited_runtime_environment() -> None:
    """Refuse inherited gateways, model credentials, and runtime homes.

    Keeping only fixture-owned configuration below makes this safe to launch
    on a developer machine that has an active gateway or cloud credentials.
    """

    prefixes = (
        "ABSTRACTGATEWAY_", "ABSTRACTCORE_", "ABSTRACTRUNTIME_", "ABSTRACTVOICE_",
        "ABSTRACTVISION_", "ABSTRACTMUSIC_", "OPENAI_", "ANTHROPIC_", "OLLAMA_",
        "GROQ_", "MISTRAL_", "GOOGLE_API_", "COHERE_", "HUGGINGFACE_", "HF_",
    )
    for key in list(os.environ):
        if key.startswith(prefixes):
            os.environ.pop(key, None)


def _node(node_id: str, node_type: str, *, pin_defaults: dict[str, Any] | None = None, effect_config: dict[str, Any] | None = None, outputs: list[dict[str, Any]] | None = None, extra_data: dict[str, Any] | None = None) -> dict[str, Any]:
    data: dict[str, Any] = {"nodeType": node_type, "label": node_type.replace("_", " ").title()}
    if pin_defaults:
        data["pinDefaults"] = pin_defaults
    if effect_config:
        data["effectConfig"] = effect_config
    if outputs is not None:
        data["outputs"] = outputs
    if extra_data:
        data.update(extra_data)
    return {"id": node_id, "type": node_type, "position": {"x": 0, "y": 0}, "data": data}


def _edge(source: str, source_handle: str, target: str, target_handle: str, edge_id: str) -> dict[str, Any]:
    return {"id": edge_id, "source": source, "sourceHandle": source_handle, "target": target, "targetHandle": target_handle}


def _prompt_flow() -> dict[str, Any]:
    """Answer -> durable ask_user -> structured on_flow_end output.

    `ticket` explicitly declares required in On Flow Start's outputs.
    `prompt` has a
    default so the fixture also demonstrates both schema shapes.
    """

    return {
        "id": "prompt-structured",
        "name": "Prompt, ask, and structured result",
        "description": "No-provider browser fixture: answer_user, ask_user, then object output.",
        "interfaces": ["chat"],
        "nodes": [
            _node("start", "on_flow_start", pin_defaults={"prompt": "Fixture prompt"}, outputs=[
                {"id": "exec-out", "label": "", "type": "execution"},
                {"id": "ticket", "label": "Ticket", "type": "string", "required": True, "schema": {"minLength": 1}},
                {"id": "prompt", "label": "Prompt", "type": "string"},
            ]),
            _node("hello", "answer_user", pin_defaults={"message": "Fixture is live. Please answer the durable question.", "level": "message"}),
            _node("ask", "ask_user", pin_defaults={"prompt": "Continue the e2e fixture?", "choices": ["continue", "stop"]}),
            _node("result", "code", extra_data={"codeBody": "return {'fixture': 'prompt-structured', 'ok': True, 'kind': 'structured-output'}"}),
            _node("end", "on_flow_end"),
        ],
        "edges": [
            _edge("start", "exec-out", "hello", "exec-in", "e1"),
            _edge("hello", "exec-out", "ask", "exec-in", "e2"),
            _edge("ask", "exec-out", "result", "exec-in", "e3"),
            _edge("result", "exec-out", "end", "exec-in", "e4"),
            _edge("result", "output", "end", "result", "e5"),
        ],
        "entryNode": "start",
    }


def _event_listener_flow() -> dict[str, Any]:
    return {
        "id": "event-listener",
        "name": "Event listener",
        "description": "Parks on a real session-scoped on_event wait.",
        "interfaces": ["event"],
        "nodes": [
            _node("listen", "on_event", extra_data={"eventConfig": {"name": "fixture.ping", "scope": "session"}}),
            _node("answer", "answer_user", pin_defaults={"message": "fixture.ping delivered", "level": "message"}),
            _node("end", "on_flow_end"),
        ],
        "edges": [
            _edge("listen", "exec-out", "answer", "exec-in", "e1"),
            _edge("answer", "exec-out", "end", "exec-in", "e2"),
        ],
        "entryNode": "listen",
    }


def _event_emitter_flow() -> dict[str, Any]:
    return {
        "id": "event-emitter",
        "name": "Event emitter",
        "description": "Emits fixture.ping through the runtime's durable event lane.",
        "interfaces": ["event"],
        "nodes": [
            _node("start", "on_flow_start"),
            _node("emit", "emit_event", pin_defaults={"payload": {"source": "abstractcode-e2e"}}, effect_config={"name": "fixture.ping", "scope": "session"}),
            _node("end", "on_flow_end"),
        ],
        "edges": [_edge("start", "exec-out", "emit", "exec-in", "e1"), _edge("emit", "exec-out", "end", "exec-in", "e2")],
        "entryNode": "start",
    }


def _tool_approval_flow() -> dict[str, Any]:
    """Native Tool Calls node with a harmless run-workspace write request.

    The gateway/runtime decides whether its installed tool executor surfaces
    this as `approval_required`; this fixture does not fake an approval wait.
    On deployments without the optional tool executor it may fail honestly,
    which is still useful to prove the UI's failure rendering.
    """

    call = {"id": "fixture-write", "name": "write_file", "arguments": {"file_path": "fixture-tool-approval.txt", "content": "e2e fixture\n"}}
    return {
        "id": "tool-approval",
        "name": "Native tool approval",
        "description": "Requests a harmless workspace write through the native Tool Calls effect.",
        "interfaces": ["tools"],
        "nodes": [
            _node("start", "on_flow_start"),
            _node("tool", "tool_calls", pin_defaults={"tool_calls": [call]}),
            _node("end", "on_flow_end"),
        ],
        "edges": [_edge("start", "exec-out", "tool", "exec-in", "e1"), _edge("tool", "exec-out", "end", "exec-in", "e2")],
        "entryNode": "start",
    }


def _assistant_contract_flow(*, authored: bool = False, generic: bool = False) -> dict[str, Any]:
    # Use the real managed orchestrator's start pins/defaults but replace its
    # model graph with a deterministic Code node. No provider is invoked.
    source = Path(__file__).resolve().parents[3] / "abstractassistant" / "abstractassistant" / "assistant_workflow.py"
    spec = importlib.util.spec_from_file_location("_assistant_contract_browser_fixture", source)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    original = module.managed_assistant_visualflow()
    start = copy.deepcopy(next(node for node in original["nodes"] if node["data"].get("nodeType") == "on_flow_start"))
    start["id"] = "start"
    start["data"]["outputs"].append({"id": "reasoning", "type": "string", "label": "Reasoning"})
    if authored:
        start["data"]["pinDefaults"].update({"provider": "fixture-authored", "model": "reasoner-authored"})
    flow_id = "typed-contract" if generic else "authored-contract" if authored else "assistant-contract"
    return {
        "id": flow_id, "name": flow_id, "interfaces": ["chat"] if generic else ["abstractassistant.agent.v1"],
        "nodes": [start, _node("result", "code", extra_data={"codeBody": "return {'answer': 'The workflow completed with its registered defaults. No model was invoked.'}"}), _node("end", "on_flow_end")],
        "edges": [_edge("start", "exec-out", "result", "exec-in", "e1"), _edge("result", "exec-out", "end", "exec-in", "e2"), _edge("result", "output", "end", "result", "e3")], "entryNode": "start",
    }


def _tool_supervision_flow() -> dict[str, Any]:
    def write(name: str) -> dict[str, Any]:
        return {"id": name, "name": "write_file", "arguments": {"file_path": f"{name}.md", "content": "# Supervision fixture\nOnly the disposable Gateway workspace is modified.\n"}}
    batches = [
        ("drafts", [write("research-notes"), write("source-index"), write("summary-draft")]),
        ("check", [{"id": "missing-source", "name": "read_file", "arguments": {"file_path": "not-created-source.md"}}, {"id": "notes", "name": "read_file", "arguments": {"file_path": "research-notes.md"}}]),
        ("save", [write("review-checklist")]),
    ]
    nodes = [_node("start", "on_flow_start")]
    nodes += [_node(name, "tool_calls", pin_defaults={"tool_calls": calls}) for name, calls in batches]
    nodes += [_node("ask", "ask_user", pin_defaults={"prompt": "Ready to save the final report?", "choices": ["continue", "stop"]}), _node("final", "tool_calls", pin_defaults={"tool_calls": [write("final-report")]}), _node("end", "on_flow_end")]
    order = ["start", "drafts", "check", "save", "ask", "final", "end"]
    return {"id": "tool-supervision", "name": "Tool supervision", "description": "Safe, isolated multi-batch approval and failed-read fixture.", "interfaces": ["tools"], "nodes": nodes, "edges": [_edge(source, "exec-out", target, "exec-in", f"e{index}") for index, (source, target) in enumerate(zip(order, order[1:]))], "entryNode": "start"}


def _basic_agent_contract_flow() -> dict[str, Any]:
    # The actual shipped Basic Agent authoring contract, not a hand-selected
    # approximation of its required/default pins. Only execution is replaced
    # with a no-model Code node, so tests cannot reach operator inference.
    source = Path(__file__).resolve().parents[3] / "abstractgateway" / "flows" / "bundles" / "basic-agent.flow"
    with zipfile.ZipFile(source) as archive:
        manifest = json.loads(archive.read("manifest.json"))
        original = json.loads(archive.read(manifest["flows"][manifest["default_entrypoint"]]))
    start = copy.deepcopy(next(node for node in original["nodes"] if node["data"].get("nodeType") == "on_flow_start"))
    start["id"] = "start"
    return {
        "id": "basic-agent-contract", "name": "Basic agent defaults", "interfaces": ["abstractcode.agent.v1"],
        "nodes": [start, _node("result", "code", extra_data={"codeBody": "return {'answer': 'Basic Agent accepted your message with automatic defaults. No model was invoked.'}"}), _node("end", "on_flow_end")],
        "edges": [_edge("start", "exec-out", "result", "exec-in", "e1"), _edge("result", "exec-out", "end", "exec-in", "e2"), _edge("result", "output", "end", "result", "e3")], "entryNode": "start",
    }


def _coding_contract_flow() -> dict[str, Any]:
    source = Path(__file__).resolve().parents[3] / "abstractgateway" / "flows" / "bundles" / "coding-agent@0.2.7.flow"
    with zipfile.ZipFile(source) as archive:
        manifest = json.loads(archive.read("manifest.json"))
        original = json.loads(archive.read(manifest["flows"]["coding-agent"]))
    start = copy.deepcopy(next(node for node in original["nodes"] if node["data"].get("nodeType") == "on_flow_start"))
    start["id"] = "start"
    return {
        "id": "coding-contract", "name": "Coding request defaults", "interfaces": ["abstractcode.coding.v1"],
        "nodes": [start, _node("result", "code", extra_data={"codeBody": "return {'answer': 'Coding workflow accepted your request with its published defaults. No model was invoked.'}"}), _node("end", "on_flow_end")],
        "edges": [_edge("start", "exec-out", "result", "exec-in", "e1"), _edge("result", "exec-out", "end", "exec-in", "e2"), _edge("result", "output", "end", "result", "e3")], "entryNode": "start",
    }


def _write_bundle(bundles: Path) -> Path:
    flows = {flow["id"]: flow for flow in (_prompt_flow(), _event_listener_flow(), _event_emitter_flow(), _tool_approval_flow(), _tool_supervision_flow(), _assistant_contract_flow(), _assistant_contract_flow(authored=True), _assistant_contract_flow(authored=True, generic=True), _basic_agent_contract_flow(), _coding_contract_flow())}
    manifest = {
        "bundle_format_version": "1", "bundle_id": BUNDLE_ID, "bundle_version": BUNDLE_VERSION,
        "created_at": "2026-09-20T00:00:00+00:00", "default_entrypoint": "prompt-structured",
        "entrypoints": [
            {"flow_id": "prompt-structured", "name": "Prompt structured", "description": "answer_user → ask_user → structured output", "interfaces": ["chat"]},
            {"flow_id": "event-listener", "name": "Event listener", "description": "on_event fixture.ping", "interfaces": ["event"]},
            {"flow_id": "event-emitter", "name": "Event emitter", "description": "emit_event fixture.ping", "interfaces": ["event"]},
            {"flow_id": "tool-approval", "name": "Native tool approval", "description": "harmless write_file Tool Calls request", "interfaces": ["tools"]},
            {"flow_id": "tool-supervision", "name": "Tool supervision", "description": "Multiple approval batches, failures, and a real question", "interfaces": ["tools"]},
            {"flow_id": "assistant-contract", "name": "Assistant contract", "description": "Real Assistant inputs; deterministic no-model execution", "interfaces": ["abstractassistant.agent.v1"]},
            {"flow_id": "authored-contract", "name": "Authored model contract", "description": "Workflow provider/model defaults override Gateway routing", "interfaces": ["abstractassistant.agent.v1"]},
            {"flow_id": "typed-contract", "name": "Typed workflow inputs", "description": "Typed controls with real Assistant input metadata", "interfaces": ["chat"]},
            {"flow_id": "basic-agent-contract", "name": "Basic agent defaults", "description": "Unchanged shipped basic-agent start pins; deterministic no-model execution", "interfaces": ["abstractcode.agent.v1"]},
            {"flow_id": "coding-contract", "name": "Coding request defaults", "description": "Published coding-agent start pins; deterministic no-model execution", "interfaces": ["abstractcode.coding.v1"]},
        ],
        "flows": {flow_id: f"flows/{flow_id}.json" for flow_id in flows}, "artifacts": {}, "assets": {},
        "metadata": {"fixture": True, "network": "loopback-only", "models": "disabled"},
    }
    path = bundles / f"{BUNDLE_ID}@{BUNDLE_VERSION}.flow"
    with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("manifest.json", json.dumps(manifest, indent=2))
        for flow_id, flow in flows.items():
            archive.writestr(f"flows/{flow_id}.json", json.dumps(flow, indent=2))
    return path


def _configure(root: Path) -> dict[str, Path]:
    _clear_inherited_runtime_environment()
    data, bundles, workspace = root / "data", root / "bundles", root / "workspace"
    for directory in (data, bundles, workspace):
        directory.mkdir(parents=True, exist_ok=True)
    # A harmless, fixture-owned shared file lets browser checks exercise the
    # real gateway browse/ingest path without touching an operator workspace.
    shared = workspace / "fixture-shared"
    shared.mkdir(exist_ok=True)
    (shared / "welcome.md").write_text(
        "# AbstractCode browser fixture\n\nThis file exists only for isolated e2e attachment checks.\n",
        encoding="utf-8",
    )
    _write_bundle(bundles)
    os.environ.update({
        "ABSTRACTGATEWAY_DATA_DIR": str(data),
        "ABSTRACTGATEWAY_FLOWS_DIR": str(bundles),
        "ABSTRACTGATEWAY_WORKFLOW_SOURCE": "bundle",
        "ABSTRACTGATEWAY_USERS_FILE": str(data / "auth" / "users.json"),
        "ABSTRACTGATEWAY_USER_AUTH": "1",
        "ABSTRACTGATEWAY_MULTI_USER": "0",
        "ABSTRACTGATEWAY_WORKSPACE_DIR": str(workspace),
        "ABSTRACTGATEWAY_POLL_S": "0.05",
        "ABSTRACTGATEWAY_TICK_WORKERS": "1",
        "ABSTRACTGATEWAY_ALLOWED_ORIGINS": "http://127.0.0.1:18782,http://localhost:18782",
        "ABSTRACTGATEWAY_DEV_ALLOW_UNAUTHENTICATED_READS": "0",
    })
    # Separate published bundle proves installed/catalog/version dedup through
    # real registry endpoints without changing the main fixture's identities.
    from abstractgateway.workflow_catalog import WorkflowCatalogStore
    catalog = WorkflowCatalogStore(root_data_dir=data)
    for version in ("1.0.0", "2.0.0"):
        flow = _coding_contract_flow()
        manifest = {
            "bundle_format_version": "1", "bundle_id": "e2e-published-coding", "bundle_version": version,
            "created_at": "2026-09-20T00:00:00+00:00", "default_entrypoint": flow["id"],
            "entrypoints": [{"flow_id": flow["id"], "name": "Published coding", "interfaces": flow["interfaces"]}],
            "flows": {flow["id"]: "flows/coding.json"}, "artifacts": {}, "assets": {},
        }
        content = io.BytesIO()
        with zipfile.ZipFile(content, "w") as archive:
            archive.writestr("manifest.json", json.dumps(manifest))
            archive.writestr("flows/coding.json", json.dumps(flow))
        (bundles / f"e2e-published-coding@{version}.flow").write_bytes(content.getvalue())
        catalog.install_bundle_bytes(content.getvalue(), make_default=version == "1.0.0", publisher="isolated-fixture")
    return {"root": root, "data": data, "bundles": bundles, "workspace": workspace}


def _mint_user() -> None:
    from abstractgateway.users import GatewayUserRegistry

    registry = GatewayUserRegistry()
    registry.create_user(user_id=USER_ID, roles=["admin"], scopes=["*"], runtime_id=USER_ID, token=TOKEN)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default=HOST)
    parser.add_argument("--port", type=int, default=DEFAULT_PORT)
    parser.add_argument("--root", type=Path, default=None, help="Optional empty fixture root. Defaults to a fresh temp directory.")
    args = parser.parse_args()
    if args.host != HOST:
        parser.error("fixture only binds 127.0.0.1")
    root = args.root.resolve() if args.root else Path(tempfile.mkdtemp(prefix="abstractcode-e2e-gateway-"))
    if args.root:
        root.mkdir(parents=True, exist_ok=True)
    paths = _configure(root)
    _mint_user()
    fixture = {
        "url": f"http://{HOST}:{args.port}", "user_id": USER_ID, "token": TOKEN,
        "bundle_id": BUNDLE_ID, "bundle_version": BUNDLE_VERSION,
        "flows": {"prompt": "prompt-structured", "listener": "event-listener", "emitter": "event-emitter", "tool_approval": "tool-approval"},
        "root": str(root), "workspace": str(paths["workspace"]),
    }
    print(json.dumps(fixture, sort_keys=True), flush=True)
    import uvicorn
    try:
        uvicorn.run("abstractgateway.app:app", host=HOST, port=args.port, log_level="warning", access_log=False)
    finally:
        # Only auto-remove roots we created. A caller-provided root is useful
        # for inspecting fixture artifacts after a failed browser assertion.
        if args.root is None:
            shutil.rmtree(root, ignore_errors=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
