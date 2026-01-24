from __future__ import annotations

import json
import re
import zipfile
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Tuple

from abstractagent.agents.base import BaseAgent
from abstractruntime import RunState, RunStatus, Runtime, WorkflowSpec


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()

_UI_EVENT_NAMESPACE = "abstract"

_STATUS_EVENT_NAME = f"{_UI_EVENT_NAMESPACE}.status"
_MESSAGE_EVENT_NAME = f"{_UI_EVENT_NAMESPACE}.message"
_TOOL_EXEC_EVENT_NAME = f"{_UI_EVENT_NAMESPACE}.tool_execution"
_TOOL_RESULT_EVENT_NAME = f"{_UI_EVENT_NAMESPACE}.tool_result"


def _normalize_ui_event_name(name: str) -> str:
    s = str(name or "").strip()
    if s.startswith("abstractcode."):
        return f"{_UI_EVENT_NAMESPACE}.{s[len('abstractcode.'):]}".strip(".")
    return s


def _new_message(*, role: str, content: str, metadata: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    from uuid import uuid4

    meta: Dict[str, Any] = dict(metadata) if isinstance(metadata, dict) else {}
    meta.setdefault("message_id", f"msg_{uuid4().hex}")
    return {
        "role": str(role or "").strip() or "user",
        "content": str(content or ""),
        "timestamp": _now_iso(),
        "metadata": meta,
    }


def _copy_messages(messages: Any) -> List[Dict[str, Any]]:
    if not isinstance(messages, list):
        return []
    out: List[Dict[str, Any]] = []
    for m in messages:
        if isinstance(m, dict):
            out.append(dict(m))
    return out


@dataclass(frozen=True)
class ResolvedVisualFlow:
    visual_flow: Dict[str, Any]
    flows: Dict[str, Dict[str, Any]]
    flows_dir: Path
    bundle_id: Optional[str] = None
    bundle_version: Optional[str] = None


def _default_flows_dir() -> Path:
    try:
        from .flow_cli import default_flows_dir

        return default_flows_dir()
    except Exception:
        return Path("flows")


def _default_bundles_dir() -> Path:
    """Best-effort location for `.flow` bundles (WorkflowBundle zips)."""
    try:
        from abstractruntime.workflow_bundle import default_workflow_bundles_dir  # type: ignore

        return default_workflow_bundles_dir()
    except Exception:
        candidate = Path("flows") / "bundles"
        if candidate.exists() and candidate.is_dir():
            return candidate
        return Path("flows")


def _is_flow_bundle(path: Path) -> bool:
    try:
        if path.suffix.lower() == ".flow":
            return True
    except Exception:
        pass
    try:
        return zipfile.is_zipfile(path)
    except Exception:
        return False


def _load_visual_flows_from_bundle(bundle_path: Path) -> Tuple[Dict[str, Dict[str, Any]], Dict[str, Any]]:
    """Load VisualFlow JSON objects from a `.flow` bundle (zip).

    Returns: (flows_by_id, manifest_dict)
    """
    try:
        from abstractruntime.workflow_bundle import open_workflow_bundle  # type: ignore
    except Exception as e:  # pragma: no cover
        raise RuntimeError(
            "AbstractRuntime workflow_bundle support is required to run `.flow` bundles.\n"
            'Install with: pip install "abstractruntime"'
        ) from e

    bundle = open_workflow_bundle(bundle_path)
    man = bundle.manifest

    entrypoints: List[Dict[str, Any]] = []
    for ep in getattr(man, "entrypoints", None) or []:
        fid = str(getattr(ep, "flow_id", "") or "").strip()
        if not fid:
            continue
        entrypoints.append(
            {
                "flow_id": fid,
                "name": str(getattr(ep, "name", "") or ""),
                "description": str(getattr(ep, "description", "") or ""),
                "interfaces": list(getattr(ep, "interfaces", []) or []),
            }
        )

    manifest: Dict[str, Any] = {
        "bundle_id": str(getattr(man, "bundle_id", "") or ""),
        "bundle_version": str(getattr(man, "bundle_version", "") or ""),
        "default_entrypoint": str(getattr(man, "default_entrypoint", "") or ""),
        "entrypoints": entrypoints,
        "flows": dict(getattr(man, "flows", None) or {}),
    }

    flows: Dict[str, Dict[str, Any]] = {}
    for fid, rel in (getattr(man, "flows", None) or {}).items():
        if not isinstance(rel, str) or not rel.strip():
            continue
        try:
            raw = bundle.read_json(rel)
        except Exception:
            continue
        if not isinstance(raw, dict):
            continue
        flow_id = str(raw.get("id") or fid or "").strip()
        if not flow_id:
            continue
        flows[flow_id] = raw
    return flows, manifest


def _load_visual_flows(flows_dir: Path) -> Dict[str, Dict[str, Any]]:
    flows: Dict[str, Dict[str, Any]] = {}
    if not flows_dir.exists():
        return flows
    for path in sorted(flows_dir.glob("*.json")):
        try:
            raw = json.loads(path.read_text(encoding="utf-8"))
        except Exception:
            continue
        if not isinstance(raw, dict):
            continue
        fid = str(raw.get("id") or "").strip()
        if not fid:
            continue
        flows[fid] = raw
    return flows


def resolve_visual_flow(
    flow_ref: str,
    *,
    flows_dir: Optional[str],
    require_interface: Optional[str] = None,
) -> ResolvedVisualFlow:
    """Resolve a VisualFlow by id, name, or path to a `.json` or bundled `.flow` file.

    Also accepts bundle refs (by bundle_id), e.g.:
      - basic-agent
      - basic-llm@0.0.2
      - basic-llm.flow
      - basic-llm@0.0.2.flow
      - basic-llm:c4bd3db6      (bundle_id:flow_id)
      - basic-llm@0.0.2:c4bd3db6
    """
    ref_raw = str(flow_ref or "").strip()
    if not ref_raw:
        raise ValueError("flow reference is required (flow id, name, .json/.flow path, or bundle id)")

    def _require_flow_interface(raw: Dict[str, Any]) -> None:
        if not require_interface:
            return
        interfaces = raw.get("interfaces")
        if isinstance(interfaces, list) and require_interface in interfaces:
            return
        raise ValueError(f"Workflow does not implement '{require_interface}'")

    ref = ref_raw
    bundle_flow_id: Optional[str] = None
    # Support bundle_id:flow_id (like AbstractCode web). Avoid clobbering Windows drive letters.
    if ":" in ref and not re.match(r"^[A-Za-z]:[\\\\/]", ref):
        left, right = ref.split(":", 1)
        if left.strip() and right.strip():
            ref = left.strip()
            bundle_flow_id = right.strip()

    path = Path(ref).expanduser()
    flows_dir_path: Path
    if path.exists() and path.is_file() and _is_flow_bundle(path):
        flows, manifest = _load_visual_flows_from_bundle(path)
        bundle_id = str(manifest.get("bundle_id") or "").strip() or None
        bundle_version = str(manifest.get("bundle_version") or "").strip() or None
        default_id = str(manifest.get("default_entrypoint") or "").strip()
        selected_id = bundle_flow_id or default_id
        if not selected_id and flows:
            selected_id = next(iter(flows.keys()))
        vf = flows.get(selected_id) if selected_id else None
        if vf is None:
            available = ", ".join(sorted(flows.keys()))
            raise ValueError(f"Bundle entrypoint '{selected_id}' not found in {path} (available: {available})")
        # Prefer bundle-level interface markers when present; fall back to flow.interfaces.
        if require_interface:
            try:
                eps = list(manifest.get("entrypoints") or [])
                ep = next(
                    (
                        e
                        for e in eps
                        if isinstance(e, dict) and str(e.get("flow_id") or "").strip() == str(selected_id)
                    ),
                    None,
                )
                if ep is None:
                    raise ValueError
                if require_interface not in list(ep.get("interfaces") or []):
                    raise ValueError
            except Exception:
                _require_flow_interface(vf)
        return ResolvedVisualFlow(
            visual_flow=vf,
            flows=flows,
            flows_dir=path.resolve(),
            bundle_id=bundle_id,
            bundle_version=bundle_version,
        )

    if path.exists() and path.is_file():
        try:
            raw = json.loads(path.read_text(encoding="utf-8"))
        except Exception as e:
            raise ValueError(f"Cannot read flow file: {path}") from e
        if not isinstance(raw, dict):
            raise ValueError(f"Flow JSON must be an object: {path}")
        flows_dir_path = Path(flows_dir).expanduser().resolve() if flows_dir else path.parent.resolve()
        flows = _load_visual_flows(flows_dir_path)
        fid = str(raw.get("id") or "").strip()
        if fid:
            flows[fid] = raw
        _require_flow_interface(raw)
        return ResolvedVisualFlow(visual_flow=raw, flows=flows, flows_dir=flows_dir_path)

    # Prefer installed bundle refs when a bundle_id (or entrypoint name) matches.
    try:
        from abstractruntime.workflow_bundle import WorkflowBundleRegistry, WorkflowBundleRegistryError  # type: ignore

        reg = WorkflowBundleRegistry(_default_bundles_dir())
        ep = reg.resolve_entrypoint(ref_raw, interface=require_interface)
        b = reg.resolve_bundle(ep.bundle_ref)
        flows2, _manifest = _load_visual_flows_from_bundle(b.path)
        vf = flows2.get(ep.flow_id)
        if vf is None:
            available = ", ".join(sorted(flows2.keys()))
            raise ValueError(f"Bundle entrypoint '{ep.flow_id}' not found in {b.path} (available: {available})")
        return ResolvedVisualFlow(
            visual_flow=vf,
            flows=flows2,
            flows_dir=b.path.resolve(),
            bundle_id=str(ep.bundle_id),
            bundle_version=str(ep.bundle_version),
        )
    except WorkflowBundleRegistryError:
        pass
    except Exception:
        pass

    flows_dir_path = Path(flows_dir).expanduser().resolve() if flows_dir else _default_flows_dir().resolve()
    flows = _load_visual_flows(flows_dir_path)

    if ref in flows:
        _require_flow_interface(flows[ref])
        return ResolvedVisualFlow(visual_flow=flows[ref], flows=flows, flows_dir=flows_dir_path)

    # Fall back to exact name match (case-insensitive).
    matches: list[Dict[str, Any]] = []
    needle = ref.casefold()
    for vf in flows.values():
        name = vf.get("name")
        if isinstance(name, str) and name.strip() and name.strip().casefold() == needle:
            matches.append(vf)

    if not matches:
        raise ValueError(f"Flow '{ref_raw}' not found in {flows_dir_path}")
    if len(matches) > 1:
        options = ", ".join([f"{str(v.get('name') or '')} ({str(v.get('id') or '')})" for v in matches])
        raise ValueError(f"Multiple flows match '{ref}': {options}")

    vf = matches[0]
    _require_flow_interface(vf)
    return ResolvedVisualFlow(visual_flow=vf, flows=flows, flows_dir=flows_dir_path)


def _tool_definitions_from_callables(tools: List[Callable[..., Any]]) -> List[Any]:
    from abstractcore.tools import ToolDefinition

    out: List[Any] = []
    for t in tools:
        tool_def = getattr(t, "_tool_definition", None) or ToolDefinition.from_function(t)
        out.append(tool_def)
    return out


def _workflow_registry() -> Any:
    try:
        from abstractruntime import WorkflowRegistry  # type: ignore

        return WorkflowRegistry()
    except Exception:  # pragma: no cover
        try:
            from abstractruntime.scheduler.registry import WorkflowRegistry  # type: ignore

            return WorkflowRegistry()
        except Exception:  # pragma: no cover

            class WorkflowRegistry(dict):  # type: ignore[no-redef]
                def register(self, workflow: Any) -> None:
                    self[str(getattr(workflow, "workflow_id", ""))] = workflow

            return WorkflowRegistry()


def _node_type_str(node: Any) -> str:
    if isinstance(node, dict):
        return str(node.get("type") or "")
    t = getattr(node, "type", None)
    return t.value if hasattr(t, "value") else str(t or "")


def _subflow_id(node: Any) -> Optional[str]:
    data = node.get("data") if isinstance(node, dict) else getattr(node, "data", None)
    if not isinstance(data, dict):
        return None
    sid = data.get("subflowId") or data.get("flowId") or data.get("workflowId") or data.get("workflow_id")
    if isinstance(sid, str) and sid.strip():
        return sid.strip()
    return None


def _compile_visual_flow_tree(
    *,
    root: Dict[str, Any],
    flows: Dict[str, Dict[str, Any]],
    tools: List[Callable[..., Any]],
    runtime: Runtime,
    bundle_id: Optional[str] = None,
    bundle_version: Optional[str] = None,
) -> Tuple[WorkflowSpec, Any]:
    from abstractruntime.visualflow_compiler import compile_visualflow
    from abstractruntime.visualflow_compiler.visual.agent_ids import visual_react_workflow_id

    # Collect referenced subflows (cycles are allowed; compile/register each id once).
    ordered_ids: List[str] = []
    seen: set[str] = set()
    queue: List[str] = [str(root.get("id") or "")]

    while queue:
        fid = queue.pop(0)
        if not fid or fid in seen:
            continue
        vf_raw = flows.get(fid)
        if vf_raw is None:
            raise ValueError(f"Subflow '{fid}' not found in loaded flows")
        seen.add(fid)
        ordered_ids.append(fid)

        for n in list(vf_raw.get("nodes") or []):
            if _node_type_str(n) != "subflow":
                continue
            sid = _subflow_id(n)
            if sid:
                queue.append(sid)

    bundle_ref = None
    if isinstance(bundle_id, str) and bundle_id.strip() and isinstance(bundle_version, str) and bundle_version.strip():
        bundle_ref = f"{bundle_id.strip()}@{bundle_version.strip()}"

    def _namespace(prefix: str, flow_id: str) -> str:
        return f"{prefix}:{flow_id}"

    def _namespace_visualflow_raw(*, raw: Dict[str, Any], prefix: str, flow_id: str, id_map: Dict[str, str]) -> Dict[str, Any]:
        def _rewrite(v: Any) -> Any:
            if isinstance(v, str):
                s = v.strip()
                return id_map.get(s) or v
            if isinstance(v, list):
                return [_rewrite(x) for x in v]
            if isinstance(v, dict):
                return {k: _rewrite(v2) for k, v2 in v.items()}
            return v

        out_any = _rewrite(raw)
        out: Dict[str, Any] = dict(out_any) if isinstance(out_any, dict) else dict(raw)
        out["id"] = id_map.get(flow_id) or _namespace(prefix, flow_id)

        nodes_raw = out.get("nodes")
        if isinstance(nodes_raw, list):
            new_nodes: list[Any] = []
            for n_any in nodes_raw:
                n = dict(n_any) if isinstance(n_any, dict) else n_any
                if isinstance(n, dict) and str(n.get("type") or "") == "agent":
                    node_id = str(n.get("id") or "").strip()
                    data = n.get("data")
                    data_d = dict(data) if isinstance(data, dict) else {}
                    cfg_raw = data_d.get("agentConfig")
                    cfg = dict(cfg_raw) if isinstance(cfg_raw, dict) else {}
                    if node_id:
                        cfg["_react_workflow_id"] = visual_react_workflow_id(flow_id=str(out.get("id") or ""), node_id=node_id)
                    data_d["agentConfig"] = cfg
                    n["data"] = data_d
                new_nodes.append(n)
            out["nodes"] = new_nodes

        return out

    registry = _workflow_registry()

    specs_by_id: Dict[str, WorkflowSpec] = {}
    id_map: Dict[str, str] = {}
    if bundle_ref:
        id_map = {fid: _namespace(bundle_ref, fid) for fid in ordered_ids}

    compiled_flows: list[Dict[str, Any]] = []
    for fid in ordered_ids:
        raw0 = flows.get(fid)
        if raw0 is None:
            continue
        raw = (
            _namespace_visualflow_raw(raw=raw0, prefix=bundle_ref, flow_id=fid, id_map=id_map)
            if bundle_ref
            else dict(raw0)
        )
        try:
            spec = compile_visualflow(raw)
        except Exception as e:
            raise RuntimeError(f"Failed compiling VisualFlow '{fid}': {e}") from e
        specs_by_id[str(spec.workflow_id)] = spec
        compiled_flows.append(raw)
        register = getattr(registry, "register", None)
        if callable(register):
            register(spec)
        else:
            registry[str(spec.workflow_id)] = spec

    # Register per-Agent-node ReAct subworkflows so visual Agent nodes can run.
    agent_nodes: List[Tuple[str, Dict[str, Any]]] = []
    for vf in compiled_flows:
        flow_id = str(vf.get("id") or "").strip()
        for n in list(vf.get("nodes") or []):
            if _node_type_str(n) != "agent":
                continue
            data = n.get("data") if isinstance(n, dict) else None
            cfg = data.get("agentConfig", {}) if isinstance(data, dict) else {}
            cfg = dict(cfg) if isinstance(cfg, dict) else {}
            wf_id_raw = cfg.get("_react_workflow_id")
            wf_id = (
                wf_id_raw.strip()
                if isinstance(wf_id_raw, str) and wf_id_raw.strip()
                else visual_react_workflow_id(flow_id=flow_id or "unknown", node_id=str((n.get("id") if isinstance(n, dict) else "") or ""))
            )
            agent_nodes.append((wf_id, cfg))

    if agent_nodes:
        from abstractagent.adapters.react_runtime import create_react_workflow
        from abstractagent.logic.builtins import (
            ASK_USER_TOOL,
            COMPACT_MEMORY_TOOL,
            DELEGATE_AGENT_TOOL,
            INSPECT_VARS_TOOL,
            OPEN_ATTACHMENT_TOOL,
            RECALL_MEMORY_TOOL,
            REMEMBER_NOTE_TOOL,
            REMEMBER_TOOL,
        )
        from abstractagent.logic.react import ReActLogic

        def _normalize_tool_names(raw: Any) -> List[str]:
            if not isinstance(raw, list):
                return []
            out: List[str] = []
            for t in raw:
                if isinstance(t, str) and t.strip():
                    out.append(t.strip())
            return out

        tool_defs = [
            ASK_USER_TOOL,
            OPEN_ATTACHMENT_TOOL,
            RECALL_MEMORY_TOOL,
            INSPECT_VARS_TOOL,
            REMEMBER_TOOL,
            REMEMBER_NOTE_TOOL,
            COMPACT_MEMORY_TOOL,
            DELEGATE_AGENT_TOOL,
            *_tool_definitions_from_callables(tools),
        ]

        for workflow_id, cfg in agent_nodes:
            tools_selected = _normalize_tool_names(cfg.get("tools"))
            logic = ReActLogic(tools=tool_defs, max_tokens=None)
            sub = create_react_workflow(
                logic=logic,
                workflow_id=workflow_id,
                provider=None,
                model=None,
                allowed_tools=tools_selected,
                on_step=None,
            )
            register = getattr(registry, "register", None)
            if callable(register):
                register(sub)
            else:
                registry[str(sub.workflow_id)] = sub

    if hasattr(runtime, "set_workflow_registry"):
        runtime.set_workflow_registry(registry)  # type: ignore[call-arg]
    else:  # pragma: no cover
        raise RuntimeError("Runtime does not support workflow registries (required for subflows/agent nodes).")

    root_id = str(root.get("id") or "")
    root_wid = id_map.get(root_id) if bundle_ref else root_id
    root_spec = specs_by_id.get(root_wid)
    if root_spec is None:
        # Shouldn't happen because root id was seeded into the queue.
        raise RuntimeError(f"Root workflow '{root_wid or root_id}' was not compiled/registered.")
    return root_spec, registry


def _apply_abstractcode_agent_v1_scaffold(flow: Dict[str, Any], *, include_recommended: bool = True) -> None:
    """Best-effort: ensure required pins exist for `abstractcode.agent.v1` flows.

    This mirrors the VisualFlow interface scaffold in `abstractflow.visual.interfaces`,
    but operates directly on raw dict JSON so AbstractCode can run bundles without
    depending on AbstractFlow.
    """
    iid = "abstractcode.agent.v1"

    interfaces = flow.get("interfaces")
    if not isinstance(interfaces, list):
        interfaces = []
        flow["interfaces"] = interfaces
    if iid not in interfaces:
        interfaces.append(iid)

    nodes = flow.get("nodes")
    if not isinstance(nodes, list):
        return

    def _ensure_node_data(node: Dict[str, Any]) -> Dict[str, Any]:
        data = node.get("data")
        if not isinstance(data, dict):
            data = {}
            node["data"] = data
        return data

    def _ensure_pin_list(data: Dict[str, Any], key: str) -> list[dict[str, Any]]:
        pins_any = data.get(key)
        if not isinstance(pins_any, list):
            pins: list[dict[str, Any]] = []
            data[key] = pins
            return pins
        if all(isinstance(p, dict) for p in pins_any):
            return pins_any  # type: ignore[return-value]
        filtered: list[dict[str, Any]] = [p for p in pins_any if isinstance(p, dict)]
        data[key] = filtered
        return filtered

    def _ensure_pin(pins: list[dict[str, Any]], *, pin_id: str, pin_type: str, label: Optional[str] = None) -> None:
        if any(isinstance(p.get("id"), str) and p.get("id") == pin_id for p in pins):
            return
        pins.append({"id": pin_id, "label": label if isinstance(label, str) else pin_id, "type": pin_type})

    start = next((n for n in nodes if isinstance(n, dict) and str(n.get("type") or "") == "on_flow_start"), None)
    if isinstance(start, dict):
        data = _ensure_node_data(start)
        outs = _ensure_pin_list(data, "outputs")
        _ensure_pin(outs, pin_id="exec-out", pin_type="execution", label="")
        _ensure_pin(outs, pin_id="provider", pin_type="provider")
        _ensure_pin(outs, pin_id="model", pin_type="model")
        _ensure_pin(outs, pin_id="prompt", pin_type="string")
        if include_recommended:
            _ensure_pin(outs, pin_id="tools", pin_type="tools")

    for end in [n for n in nodes if isinstance(n, dict) and str(n.get("type") or "") == "on_flow_end"]:
        data = _ensure_node_data(end)
        ins = _ensure_pin_list(data, "inputs")
        _ensure_pin(ins, pin_id="exec-in", pin_type="execution", label="")
        _ensure_pin(ins, pin_id="response", pin_type="string")
        _ensure_pin(ins, pin_id="success", pin_type="boolean")
        _ensure_pin(ins, pin_id="meta", pin_type="object")
        if include_recommended:
            _ensure_pin(ins, pin_id="scratchpad", pin_type="object")


def _validate_abstractcode_agent_v1(flow: Dict[str, Any]) -> List[str]:
    """Minimal contract validation for `abstractcode.agent.v1` workflows (stdlib-only)."""
    errors: list[str] = []
    nodes = flow.get("nodes")
    if not isinstance(nodes, list):
        return ["Flow.nodes must be a list"]

    def _pins(node: Dict[str, Any], key: str) -> Optional[set[str]]:
        data = node.get("data")
        if not isinstance(data, dict):
            return None
        pins = data.get(key)
        if not isinstance(pins, list):
            return None
        out: set[str] = set()
        for p in pins:
            if not isinstance(p, dict):
                continue
            if p.get("type") == "execution":
                continue
            pid = p.get("id")
            if isinstance(pid, str) and pid.strip():
                out.add(pid.strip())
        return out

    start = next((n for n in nodes if isinstance(n, dict) and str(n.get("type") or "") == "on_flow_start"), None)
    if not isinstance(start, dict):
        errors.append("Missing On Flow Start node (type=on_flow_start)")
    else:
        outs = _pins(start, "outputs")
        required_out = {"prompt", "provider", "model", "tools"}
        if outs is not None and not required_out.issubset(outs):
            missing = ", ".join(sorted(required_out - outs))
            errors.append(f"On Flow Start outputs missing: {missing}")

    end = next((n for n in nodes if isinstance(n, dict) and str(n.get("type") or "") == "on_flow_end"), None)
    if not isinstance(end, dict):
        errors.append("Missing On Flow End node (type=on_flow_end)")
    else:
        ins = _pins(end, "inputs")
        required_in = {"response", "success", "meta"}
        if ins is not None and not required_in.issubset(ins):
            missing = ", ".join(sorted(required_in - ins))
            errors.append(f"On Flow End inputs missing: {missing}")

    return errors


class WorkflowAgent(BaseAgent):
    """Run a VisualFlow workflow as a RunnableFlow in AbstractCode.

    Contract: the workflow must declare `interfaces: ["abstractcode.agent.v1"]` and expose:
    - On Flow Start output pins (required): `provider` (provider), `model` (model), `prompt` (string)
    - On Flow End input pins (required): `response` (string), `success` (boolean), `meta` (object)
    """

    def __init__(
        self,
        *,
        runtime: Runtime,
        flow_ref: str,
        flows_dir: Optional[str] = None,
        tools: Optional[List[Callable[..., Any]]] = None,
        on_step: Optional[Callable[[str, Dict[str, Any]], None]] = None,
        max_iterations: int = 25,
        max_tokens: Optional[int] = None,
        actor_id: Optional[str] = None,
        session_id: Optional[str] = None,
    ):
        self._max_iterations = int(max_iterations) if isinstance(max_iterations, int) else 25
        if self._max_iterations < 1:
            self._max_iterations = 1
        self._max_tokens = max_tokens
        self._flow_ref = str(flow_ref or "").strip()
        if not self._flow_ref:
            raise ValueError("flow_ref is required")

        ABSTRACTCODE_AGENT_V1 = "abstractcode.agent.v1"
        resolved = resolve_visual_flow(self._flow_ref, flows_dir=flows_dir, require_interface=ABSTRACTCODE_AGENT_V1)
        self.visual_flow = resolved.visual_flow
        self.flows = resolved.flows
        self.flows_dir = resolved.flows_dir
        self._bundle_id = resolved.bundle_id
        self._bundle_version = resolved.bundle_version

        _apply_abstractcode_agent_v1_scaffold(self.visual_flow, include_recommended=True)

        errors = _validate_abstractcode_agent_v1(self.visual_flow)
        if errors:
            joined = "\n".join([f"- {e}" for e in errors])
            raise ValueError(f"Workflow does not implement '{ABSTRACTCODE_AGENT_V1}':\n{joined}")

        self._last_task: Optional[str] = None
        self._ledger_unsubscribe: Optional[Callable[[], None]] = None
        self._node_labels_by_id: Dict[str, str] = {}

        super().__init__(
            runtime=runtime,
            tools=tools,
            on_step=on_step,
            actor_id=actor_id,
            session_id=session_id,
        )

    def _create_workflow(self) -> WorkflowSpec:
        tools = list(self.tools or [])
        spec, _registry = _compile_visual_flow_tree(
            root=self.visual_flow,
            flows=self.flows,
            tools=tools,
            runtime=self.runtime,
            bundle_id=self._bundle_id,
            bundle_version=self._bundle_version,
        )
        return spec

    def start(
        self,
        task: str,
        *,
        allowed_tools: Optional[List[str]] = None,
        attachments: Optional[List[Any]] = None,
        **_: Any,
    ) -> str:
        task = str(task or "").strip()
        if not task:
            raise ValueError("task must be a non-empty string")

        self._last_task = task

        try:
            base_limits = dict(self.runtime.config.to_limits_dict())
        except Exception:
            base_limits = {}
        limits: Dict[str, Any] = dict(base_limits)
        limits.setdefault("warn_iterations_pct", 80)
        limits.setdefault("warn_tokens_pct", 80)
        limits["max_iterations"] = int(self._max_iterations)
        limits["current_iteration"] = 0
        limits.setdefault("max_history_messages", -1)
        limits.setdefault("estimated_tokens_used", 0)
        if self._max_tokens is not None:
            try:
                mt = int(self._max_tokens)
            except Exception:
                mt = None
            if isinstance(mt, int) and mt > 0:
                limits["max_tokens"] = mt

        runtime_provider = getattr(getattr(self.runtime, "config", None), "provider", None)
        runtime_model = getattr(getattr(self.runtime, "config", None), "model", None)

        vars: Dict[str, Any] = {
            "prompt": task,
            "context": {"task": task, "messages": _copy_messages(self.session_messages)},
            "_temp": {},
            "_limits": limits,
        }
        if attachments:
            items = list(attachments) if isinstance(attachments, tuple) else attachments if isinstance(attachments, list) else []
            normalized: list[Any] = []
            for a in items:
                if isinstance(a, dict):
                    normalized.append(dict(a))
                elif isinstance(a, str) and a.strip():
                    normalized.append(a.strip())
            if normalized:
                ctx = vars.get("context")
                if not isinstance(ctx, dict):
                    ctx = {"task": task, "messages": _copy_messages(self.session_messages)}
                    vars["context"] = ctx
                ctx["attachments"] = normalized

        if isinstance(runtime_provider, str) and runtime_provider.strip():
            vars["provider"] = runtime_provider.strip()
        if isinstance(runtime_model, str) and runtime_model.strip():
            vars["model"] = runtime_model.strip()

        if isinstance(allowed_tools, list):
            normalized = [str(t).strip() for t in allowed_tools if isinstance(t, str) and t.strip()]
            vars["tools"] = normalized
            vars["_runtime"] = {"allowed_tools": normalized}
        else:
            # Provide a safe default so interface-scaffolded `tools` pins resolve.
            vars["tools"] = []

        actor_id = self._ensure_actor_id()
        session_id = self._ensure_session_id()

        run_id = self.runtime.start(
            workflow=self.workflow,
            vars=vars,
            actor_id=actor_id,
            session_id=session_id,
        )
        self._current_run_id = run_id

        # Build a stable node_id -> label map for UX (used for status updates).
        try:
            labels: Dict[str, str] = {}
            for n in list(self.visual_flow.get("nodes") or []):
                if not isinstance(n, dict):
                    continue
                nid = n.get("id")
                if not isinstance(nid, str) or not nid:
                    continue
                data = n.get("data")
                label = data.get("label") if isinstance(data, dict) else None
                if isinstance(label, str) and label.strip():
                    labels[nid] = label.strip()
            self._node_labels_by_id = labels
        except Exception:
            self._node_labels_by_id = {}

        # Subscribe to ledger records so we can surface real-time status updates
        # even while a blocking effect (LLM/tool HTTP) is in-flight.
        self._ledger_unsubscribe = None
        if self.on_step:
            try:
                self._ledger_unsubscribe = self._subscribe_ui_events(actor_id=actor_id, session_id=session_id)
            except Exception:
                self._ledger_unsubscribe = None

        if self.on_step:
            try:
                self.on_step(
                    "init",
                    {
                        "flow_id": str(self.visual_flow.get("id") or ""),
                        "flow_name": str(self.visual_flow.get("name") or ""),
                        "bundle_id": self._bundle_id,
                        "bundle_version": self._bundle_version,
                    },
                )
            except Exception:
                pass

        return run_id

    def _subscribe_ui_events(self, *, actor_id: str, session_id: str) -> Optional[Callable[[], None]]:
        """Subscribe to ledger appends and translate reserved workflow UX events into on_step(...).

        This is best-effort and must never affect correctness.
        """

        def _extract_text(payload: Any) -> str:
            if isinstance(payload, str):
                return payload
            if isinstance(payload, dict):
                v0 = payload.get("value")
                if isinstance(v0, str) and v0.strip():
                    return v0.strip()
                for k in ("text", "message", "status"):
                    v = payload.get(k)
                    if isinstance(v, str) and v.strip():
                        return v.strip()
            return ""

        def _extract_duration_seconds(payload: Any) -> Optional[float]:
            if not isinstance(payload, dict):
                return None
            raw = payload.get("duration")
            if raw is None:
                raw = payload.get("duration_s")
            if raw is None:
                return None
            try:
                return float(raw)
            except Exception:
                return None

        def _extract_status(payload: Any) -> Dict[str, Any]:
            if isinstance(payload, str):
                return {"text": payload}
            if isinstance(payload, dict):
                text = _extract_text(payload)
                out: Dict[str, Any] = {"text": text}
                dur = _extract_duration_seconds(payload)
                if dur is not None:
                    out["duration"] = dur
                return out
            return {"text": str(payload or "")}

        def _extract_message(payload: Any) -> Dict[str, Any]:
            if isinstance(payload, str):
                return {"text": payload}
            if isinstance(payload, dict):
                text = _extract_text(payload)
                out: Dict[str, Any] = {"text": text}
                level = payload.get("level")
                if isinstance(level, str) and level.strip():
                    out["level"] = level.strip().lower()
                title = payload.get("title")
                if isinstance(title, str) and title.strip():
                    out["title"] = title.strip()
                meta = payload.get("meta")
                if isinstance(meta, dict):
                    out["meta"] = dict(meta)
                return out
            return {"text": str(payload or "")}

        def _extract_tool_exec(payload: Any) -> Dict[str, Any]:
            if isinstance(payload, str):
                return {"tool": payload, "args": {}}
            if isinstance(payload, dict):
                # Support both AbstractCore-normalized tool call shapes and common OpenAI-style shapes.
                #
                # Normalized (preferred):
                #   {"name": "...", "arguments": {...}, "call_id": "..."}
                #
                # OpenAI-ish:
                #   {"id": "...", "type":"function", "function":{"name":"...", "arguments":"{...json...}"}}
                tool = payload.get("tool") or payload.get("name") or payload.get("tool_name")
                args = payload.get("arguments")
                if args is None:
                    args = payload.get("args")
                call_id = payload.get("call_id") or payload.get("callId") or payload.get("id")

                fn = payload.get("function")
                if tool is None and isinstance(fn, dict):
                    tool = fn.get("name")
                if args is None and isinstance(fn, dict):
                    args = fn.get("arguments")

                parsed_args: Dict[str, Any] = {}
                if isinstance(args, dict):
                    parsed_args = dict(args)
                elif isinstance(args, str) and args.strip():
                    # Some providers send JSON arguments as a string.
                    try:
                        parsed = json.loads(args)
                        if isinstance(parsed, dict):
                            parsed_args = parsed
                    except Exception:
                        parsed_args = {}

                out: Dict[str, Any] = {"tool": str(tool or "tool"), "args": parsed_args}
                if isinstance(call_id, str) and call_id.strip():
                    out["call_id"] = call_id.strip()
                return out
            return {"tool": "tool", "args": {}}

        def _extract_tool_result(payload: Any) -> Dict[str, Any]:
            # Normalize to ReactShell's existing "observe" step contract:
            #   {tool, result (string), success?}
            tool = "tool"
            success = None
            result_str = ""
            if isinstance(payload, dict):
                tool_raw = payload.get("tool") or payload.get("name") or payload.get("tool_name")
                if isinstance(tool_raw, str) and tool_raw.strip():
                    tool = tool_raw.strip()
                if "success" in payload:
                    try:
                        success = bool(payload.get("success"))
                    except Exception:
                        success = None
                # Prefer output/result; fallback to error/value.
                raw = payload.get("output")
                if raw is None:
                    raw = payload.get("result")
                if raw is None:
                    raw = payload.get("error")
                if raw is None:
                    raw = payload.get("value")
                if raw is None:
                    raw = ""
                if isinstance(raw, str):
                    result_str = raw
                else:
                    try:
                        result_str = json.dumps(raw, ensure_ascii=False, sort_keys=True, indent=2)
                    except Exception:
                        result_str = str(raw)
            elif isinstance(payload, str):
                result_str = payload
            else:
                result_str = str(payload or "")
            out: Dict[str, Any] = {"tool": tool, "result": result_str}
            if success is not None:
                out["success"] = success
            return out

        def _on_record(rec: Dict[str, Any]) -> None:
            try:
                if rec.get("actor_id") != actor_id:
                    return
                if rec.get("session_id") != session_id:
                    return
                status = rec.get("status")
                status_str = status.value if hasattr(status, "value") else str(status or "")
                if status_str != "completed":
                    return
                eff = rec.get("effect")
                if not isinstance(eff, dict) or str(eff.get("type") or "") != "emit_event":
                    return
                payload = eff.get("payload") if isinstance(eff.get("payload"), dict) else {}
                name = str(payload.get("name") or payload.get("event_name") or "").strip()
                if not name:
                    return
                name = _normalize_ui_event_name(name)

                event_payload = payload.get("payload")
                if name == _STATUS_EVENT_NAME:
                    st = _extract_status(event_payload)
                    if callable(self.on_step) and str(st.get("text") or "").strip():
                        self.on_step("status", st)
                    return

                if name == _MESSAGE_EVENT_NAME:
                    msg = _extract_message(event_payload)
                    if callable(self.on_step) and str(msg.get("text") or "").strip():
                        self.on_step("message", msg)
                    return

                if name == _TOOL_EXEC_EVENT_NAME:
                    # Backwards-compatible: older emit_event nodes wrapped non-dict payloads under {"value": ...}.
                    raw_tc_payload = event_payload
                    if isinstance(raw_tc_payload, dict) and isinstance(raw_tc_payload.get("value"), list):
                        raw_tc_payload = raw_tc_payload.get("value")

                    if isinstance(raw_tc_payload, list):
                        for item in raw_tc_payload:
                            tc = _extract_tool_exec(item)
                            if callable(self.on_step) and str(tc.get("tool") or "").strip():
                                # Reuse AbstractCode's existing "tool call" UX.
                                self.on_step("act", tc)
                    else:
                        tc = _extract_tool_exec(raw_tc_payload)
                        if callable(self.on_step) and str(tc.get("tool") or "").strip():
                            # Reuse AbstractCode's existing "tool call" UX.
                            self.on_step("act", tc)
                    return

                if name == _TOOL_RESULT_EVENT_NAME:
                    raw_tr_payload = event_payload
                    if isinstance(raw_tr_payload, dict) and isinstance(raw_tr_payload.get("value"), list):
                        raw_tr_payload = raw_tr_payload.get("value")

                    if isinstance(raw_tr_payload, list):
                        for item in raw_tr_payload:
                            tr = _extract_tool_result(item)
                            if callable(self.on_step):
                                # Reuse AbstractCode's existing "tool result" UX.
                                self.on_step("observe", tr)
                    else:
                        tr = _extract_tool_result(raw_tr_payload)
                        if callable(self.on_step):
                            # Reuse AbstractCode's existing "tool result" UX.
                            self.on_step("observe", tr)
                    return
            except Exception:
                return

        try:
            unsub = self.runtime.subscribe_ledger(_on_record, run_id=None)
            return unsub if callable(unsub) else None
        except Exception:
            return None

    def _cleanup_ledger_subscription(self) -> None:
        unsub = self._ledger_unsubscribe
        self._ledger_unsubscribe = None
        if callable(unsub):
            try:
                unsub()
            except Exception:
                pass

    def _auto_wait_until(self, state: RunState) -> Optional[RunState]:
        """Best-effort: auto-drive short WAIT_UNTIL delays for workflow agents.

        Why:
        - Visual workflows commonly use Delay (WAIT_UNTIL) for UX pacing.
        - AbstractCode's agent run loop expects `step()` to keep making progress without
          manual `/resume` for short waits.

        Notes:
        - This is intentionally conservative: it yields back if the wait changes to a
          different reason (tool approvals, user prompts, pauses).
        - Cancellation/pause are polled so control-plane actions remain responsive.
        """
        waiting = getattr(state, "waiting", None)
        if waiting is None:
            return None

        reason = getattr(waiting, "reason", None)
        reason_value = reason.value if hasattr(reason, "value") else str(reason or "")
        if reason_value != "until":
            return None

        until_raw = getattr(waiting, "until", None)
        if not isinstance(until_raw, str) or not until_raw.strip():
            return None

        def _parse_until_iso(value: str) -> Optional[datetime]:
            s = str(value or "").strip()
            if not s:
                return None
            # Accept both "+00:00" and "Z"
            if s.endswith("Z"):
                s = s[:-1] + "+00:00"
            try:
                dt = datetime.fromisoformat(s)
            except Exception:
                return None
            if dt.tzinfo is None:
                dt = dt.replace(tzinfo=timezone.utc)
            return dt.astimezone(timezone.utc)

        until_dt = _parse_until_iso(until_raw)
        if until_dt is None:
            return None

        import time

        # Cap auto-wait to avoid surprising "hangs" for long schedules.
        max_auto_wait_s = 30.0

        while True:
            try:
                latest = self.runtime.get_state(state.run_id)
            except Exception:
                latest = state

            # Stop if externally controlled or otherwise no longer a time wait.
            if getattr(latest, "status", None) in (RunStatus.CANCELLED, RunStatus.FAILED, RunStatus.COMPLETED):
                return latest

            latest_wait = getattr(latest, "waiting", None)
            if latest_wait is None:
                return latest
            r = getattr(latest_wait, "reason", None)
            r_val = r.value if hasattr(r, "value") else str(r or "")
            if r_val != "until":
                # Another wait type (pause/user/tool/event/subworkflow) should be handled by the host.
                return latest

            now = datetime.now(timezone.utc)
            remaining = (until_dt - now).total_seconds()
            if remaining <= 0:
                # Runtime.tick will auto-unblock on the next call.
                return None

            if remaining > max_auto_wait_s:
                # Leave it waiting; user can /resume later.
                return latest

            time.sleep(min(0.25, max(0.0, float(remaining))))

    def _auto_drive_subworkflow_wait(self, state: RunState) -> Optional[RunState]:
        """Best-effort: drive async SUBWORKFLOW waits for non-interactive hosts.

        Visual subflow nodes are compiled into async+wait subworkflow effects so
        interactive hosts (e.g. web) can stream nested runs. AbstractCode's agent
        loop expects `step()` to keep progressing without needing an external
        sub-run driver, so we tick sub-runs and bubble their completions up.
        """
        from abstractruntime.core.models import WaitReason

        waiting = getattr(state, "waiting", None)
        if waiting is None or getattr(waiting, "reason", None) != WaitReason.SUBWORKFLOW:
            return None

        top_run_id = str(getattr(state, "run_id", "") or "")
        if not top_run_id:
            return None

        def _extract_sub_run_id(wait_state: object) -> Optional[str]:
            details = getattr(wait_state, "details", None)
            if isinstance(details, dict):
                sub_run_id = details.get("sub_run_id")
                if isinstance(sub_run_id, str) and sub_run_id:
                    return sub_run_id
            wait_key = getattr(wait_state, "wait_key", None)
            if isinstance(wait_key, str) and wait_key.startswith("subworkflow:"):
                return wait_key.split("subworkflow:", 1)[1] or None
            return None

        def _workflow_for(run_state: object) -> Any:
            reg = getattr(self.runtime, "workflow_registry", None)
            getter = getattr(reg, "get", None) if reg is not None else None
            if callable(getter):
                wf = getter(getattr(run_state, "workflow_id", ""))
                if wf is not None:
                    return wf
            if getattr(self.workflow, "workflow_id", None) == getattr(run_state, "workflow_id", None):
                return self.workflow
            raise RuntimeError(f"Workflow '{getattr(run_state, 'workflow_id', '')}' not found in runtime registry")

        def _bubble_completion(child_state: object) -> Optional[str]:
            parent_id = getattr(child_state, "parent_run_id", None)
            if not isinstance(parent_id, str) or not parent_id:
                return None
            parent_state = self.runtime.get_state(parent_id)
            parent_wait = getattr(parent_state, "waiting", None)
            if parent_state.status != RunStatus.WAITING or parent_wait is None:
                return None
            if parent_wait.reason != WaitReason.SUBWORKFLOW:
                return None
            self.runtime.resume(
                workflow=_workflow_for(parent_state),
                run_id=parent_id,
                wait_key=None,
                payload={
                    "sub_run_id": getattr(child_state, "run_id", None),
                    "output": getattr(child_state, "output", None),
                    "node_traces": self.runtime.get_node_traces(getattr(child_state, "run_id", "")),
                },
                max_steps=0,
            )
            return parent_id

        # Drive subruns until we either make progress or hit a non-subworkflow wait.
        for _ in range(200):
            # Descend to the deepest sub-run referenced by SUBWORKFLOW waits.
            current_run_id = top_run_id
            for _ in range(25):
                cur_state = self.runtime.get_state(current_run_id)
                cur_wait = getattr(cur_state, "waiting", None)
                if cur_state.status != RunStatus.WAITING or cur_wait is None:
                    break
                if cur_wait.reason != WaitReason.SUBWORKFLOW:
                    break
                next_id = _extract_sub_run_id(cur_wait)
                if not next_id:
                    break
                current_run_id = next_id

            current_state = self.runtime.get_state(current_run_id)

            # Tick running subruns until they block/complete.
            if current_state.status == RunStatus.RUNNING:
                current_state = self.runtime.tick(
                    workflow=_workflow_for(current_state),
                    run_id=current_run_id,
                    max_steps=100,
                )

            if current_state.status == RunStatus.RUNNING:
                continue

            if current_state.status in (RunStatus.FAILED, RunStatus.CANCELLED):
                return current_state

            if current_state.status == RunStatus.WAITING:
                cur_wait = getattr(current_state, "waiting", None)
                if cur_wait is None:
                    break
                if cur_wait.reason == WaitReason.SUBWORKFLOW:
                    continue
                # Blocked on a real wait (USER/EVENT/UNTIL/...): stop auto-driving.
                return self.runtime.get_state(top_run_id)

            if current_state.status == RunStatus.COMPLETED:
                parent_id = _bubble_completion(current_state)
                if parent_id is None:
                    return self.runtime.get_state(top_run_id)
                continue

        return self.runtime.get_state(top_run_id)

    def step(self) -> RunState:
        if not self._current_run_id:
            raise RuntimeError("No active run. Call start() first.")

        state = self.runtime.tick(workflow=self.workflow, run_id=self._current_run_id, max_steps=1)

        # Auto-drive short time waits (Delay node) so workflow agents can use pacing
        # without requiring manual `/resume`.
        if state.status == RunStatus.WAITING:
            advanced = self._auto_wait_until(state)
            if isinstance(advanced, RunState):
                state = advanced
            elif advanced is None:
                # Time passed (or will pass within our polling loop): continue ticking once.
                state = self.runtime.tick(workflow=self.workflow, run_id=self._current_run_id, max_steps=1)

        if state.status == RunStatus.WAITING:
            driven = self._auto_drive_subworkflow_wait(state)
            if isinstance(driven, RunState):
                state = driven

        if state.status == RunStatus.COMPLETED:
            response_text = ""
            meta_out: Dict[str, Any] = {}
            scratchpad_out: Any = None
            workflow_success: Optional[bool] = None
            out = getattr(state, "output", None)
            if isinstance(out, dict):
                def _pick_textish(value: Any) -> str:
                    if isinstance(value, str):
                        return value.strip()
                    if value is None:
                        return ""
                    if isinstance(value, bool):
                        return str(value).lower()
                    if isinstance(value, (int, float)):
                        return str(value)
                    return ""

                payload = out.get("result") if isinstance(out.get("result"), dict) else out

                response_text = _pick_textish(payload.get("response"))
                if not response_text:
                    response_text = (
                        _pick_textish(payload.get("answer"))
                        or _pick_textish(payload.get("message"))
                        or _pick_textish(payload.get("text"))
                        or _pick_textish(payload.get("content"))
                    )
                if not response_text and isinstance(out.get("result"), str):
                    response_text = str(out.get("result") or "").strip()

                if isinstance(payload.get("success"), bool):
                    workflow_success = bool(payload.get("success"))

                raw_meta = payload.get("meta")
                if isinstance(raw_meta, dict):
                    meta_out = dict(raw_meta)
                scratchpad_out = payload.get("scratchpad")
                if scratchpad_out is None and isinstance(out.get("scratchpad"), (dict, list, str, int, float, bool)):
                    scratchpad_out = out.get("scratchpad")

                # Backward-compat: older runs used meta.success instead of a first-class pin.
                if workflow_success is None and isinstance(meta_out.get("success"), bool):
                    workflow_success = bool(meta_out.get("success"))

                # Fallback: if the workflow doesn't expose success, treat run completion as success.
                if workflow_success is None and isinstance(out.get("success"), bool):
                    workflow_success = bool(out.get("success"))
                if workflow_success is None:
                    workflow_success = True

            task = str(self._last_task or "")
            ctx = state.vars.get("context") if isinstance(getattr(state, "vars", None), dict) else None
            if not isinstance(ctx, dict):
                ctx = {"task": task, "messages": []}
                state.vars["context"] = ctx

            msgs_raw = ctx.get("messages")
            msgs = _copy_messages(msgs_raw)
            msgs.append(_new_message(role="user", content=task))

            assistant_meta: Dict[str, Any] = {}
            if meta_out:
                assistant_meta["workflow_meta"] = meta_out
            if scratchpad_out is not None:
                assistant_meta["workflow_scratchpad"] = scratchpad_out
            if workflow_success is not None:
                assistant_meta["workflow_success"] = workflow_success

            msgs.append(_new_message(role="assistant", content=response_text, metadata=assistant_meta))
            ctx["messages"] = msgs

            # Persist best-effort so restarts can load history from run state.
            store = getattr(self.runtime, "run_store", None) or getattr(self.runtime, "_run_store", None)
            save = getattr(store, "save", None)
            if callable(save):
                try:
                    save(state)
                except Exception:
                    pass

            self.session_messages = list(msgs)

            if self.on_step:
                try:
                    self.on_step(
                        "done",
                        {
                            "answer": response_text,
                            "success": workflow_success,
                            "meta": meta_out or None,
                            "scratchpad": scratchpad_out,
                        },
                    )
                except Exception:
                    pass
            self._cleanup_ledger_subscription()

        if state.status in (RunStatus.FAILED, RunStatus.CANCELLED):
            self._sync_session_caches_from_state(state)
            self._cleanup_ledger_subscription()

        return state


def dump_visual_flow_json(flow: Any) -> str:
    """Debug helper for printing a VisualFlow as JSON (used in tests)."""
    try:
        return flow.model_dump_json(indent=2)
    except Exception:
        try:
            data = flow.model_dump()
        except Exception:
            data = {}
        return json.dumps(data, indent=2, ensure_ascii=False, default=str)
