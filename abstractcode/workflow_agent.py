from __future__ import annotations

import json
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Tuple

from abstractagent.agents.base import BaseAgent
from abstractruntime import RunState, RunStatus, Runtime, WorkflowSpec


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def _new_message(*, role: str, content: str) -> Dict[str, Any]:
    from uuid import uuid4

    return {
        "role": str(role or "").strip() or "user",
        "content": str(content or ""),
        "timestamp": _now_iso(),
        "metadata": {"message_id": f"msg_{uuid4().hex}"},
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
    visual_flow: Any
    flows: Dict[str, Any]
    flows_dir: Path


def _default_flows_dir() -> Path:
    try:
        from .flow_cli import default_flows_dir

        return default_flows_dir()
    except Exception:
        return Path("flows")


def _load_visual_flows(flows_dir: Path) -> Dict[str, Any]:
    try:
        from abstractflow.visual.models import VisualFlow
    except Exception as e:  # pragma: no cover
        raise RuntimeError(
            "AbstractFlow is required to run VisualFlow workflows.\n"
            'Install with: pip install "abstractcode[flow]"'
        ) from e

    flows: Dict[str, Any] = {}
    if not flows_dir.exists():
        return flows
    for path in sorted(flows_dir.glob("*.json")):
        try:
            raw = path.read_text(encoding="utf-8")
            vf = VisualFlow.model_validate_json(raw)
        except Exception:
            continue
        flows[str(vf.id)] = vf
    return flows


def resolve_visual_flow(flow_ref: str, *, flows_dir: Optional[str]) -> ResolvedVisualFlow:
    """Resolve a VisualFlow by id, name, or path to a .json file."""
    ref = str(flow_ref or "").strip()
    if not ref:
        raise ValueError("flow reference is required (flow id, name, or .json path)")

    path = Path(ref).expanduser()
    flows_dir_path: Path
    if path.exists() and path.is_file():
        try:
            raw = path.read_text(encoding="utf-8")
        except Exception as e:
            raise ValueError(f"Cannot read flow file: {path}") from e

        try:
            from abstractflow.visual.models import VisualFlow
        except Exception as e:  # pragma: no cover
            raise RuntimeError(
                "AbstractFlow is required to run VisualFlow workflows.\n"
                'Install with: pip install "abstractcode[flow]"'
            ) from e

        vf = VisualFlow.model_validate_json(raw)
        flows_dir_path = Path(flows_dir).expanduser().resolve() if flows_dir else path.parent.resolve()
        flows = _load_visual_flows(flows_dir_path)
        flows[str(vf.id)] = vf
        return ResolvedVisualFlow(visual_flow=vf, flows=flows, flows_dir=flows_dir_path)

    flows_dir_path = Path(flows_dir).expanduser().resolve() if flows_dir else _default_flows_dir().resolve()
    flows = _load_visual_flows(flows_dir_path)

    if ref in flows:
        return ResolvedVisualFlow(visual_flow=flows[ref], flows=flows, flows_dir=flows_dir_path)

    # Fall back to exact name match (case-insensitive).
    matches = []
    needle = ref.casefold()
    for vf in flows.values():
        name = getattr(vf, "name", None)
        if isinstance(name, str) and name.strip() and name.strip().casefold() == needle:
            matches.append(vf)

    if not matches:
        raise ValueError(f"Flow '{ref}' not found in {flows_dir_path}")
    if len(matches) > 1:
        options = ", ".join([f"{getattr(v, 'name', '')} ({getattr(v, 'id', '')})" for v in matches])
        raise ValueError(f"Multiple flows match '{ref}': {options}")

    vf = matches[0]
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
    t = getattr(node, "type", None)
    return t.value if hasattr(t, "value") else str(t or "")


def _subflow_id(node: Any) -> Optional[str]:
    data = getattr(node, "data", None)
    if not isinstance(data, dict):
        return None
    sid = data.get("subflowId") or data.get("flowId") or data.get("workflowId") or data.get("workflow_id")
    if isinstance(sid, str) and sid.strip():
        return sid.strip()
    return None


def _compile_visual_flow_tree(
    *,
    root: Any,
    flows: Dict[str, Any],
    tools: List[Callable[..., Any]],
    runtime: Runtime,
) -> Tuple[WorkflowSpec, Any]:
    from abstractflow.compiler import compile_flow
    from abstractflow.visual.agent_ids import visual_react_workflow_id
    from abstractflow.visual.executor import visual_to_flow

    # Collect referenced subflows (cycles are allowed; compile/register each id once).
    ordered: List[Any] = []
    seen: set[str] = set()
    queue: List[str] = [str(getattr(root, "id", "") or "")]

    while queue:
        fid = queue.pop(0)
        if not fid or fid in seen:
            continue
        vf = flows.get(fid)
        if vf is None:
            raise ValueError(f"Subflow '{fid}' not found in loaded flows")
        seen.add(fid)
        ordered.append(vf)

        for n in getattr(vf, "nodes", []) or []:
            if _node_type_str(n) != "subflow":
                continue
            sid = _subflow_id(n)
            if sid:
                queue.append(sid)

    registry = _workflow_registry()

    specs_by_id: Dict[str, WorkflowSpec] = {}
    for vf in ordered:
        f = visual_to_flow(vf)
        spec = compile_flow(f)
        specs_by_id[str(spec.workflow_id)] = spec
        register = getattr(registry, "register", None)
        if callable(register):
            register(spec)
        else:
            registry[str(spec.workflow_id)] = spec

    # Register per-Agent-node ReAct subworkflows so visual Agent nodes can run.
    agent_nodes: List[Tuple[str, Dict[str, Any]]] = []
    for vf in ordered:
        for n in getattr(vf, "nodes", []) or []:
            if _node_type_str(n) != "agent":
                continue
            data = getattr(n, "data", None)
            cfg = data.get("agentConfig", {}) if isinstance(data, dict) else {}
            cfg = dict(cfg) if isinstance(cfg, dict) else {}
            wf_id_raw = cfg.get("_react_workflow_id")
            wf_id = (
                wf_id_raw.strip()
                if isinstance(wf_id_raw, str) and wf_id_raw.strip()
                else visual_react_workflow_id(flow_id=vf.id, node_id=n.id)
            )
            agent_nodes.append((wf_id, cfg))

    if agent_nodes:
        from abstractagent.adapters.react_runtime import create_react_workflow
        from abstractagent.logic.builtins import (
            ASK_USER_TOOL,
            COMPACT_MEMORY_TOOL,
            INSPECT_VARS_TOOL,
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
            RECALL_MEMORY_TOOL,
            INSPECT_VARS_TOOL,
            REMEMBER_TOOL,
            REMEMBER_NOTE_TOOL,
            COMPACT_MEMORY_TOOL,
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

    root_id = str(getattr(root, "id", "") or "")
    root_spec = specs_by_id.get(root_id)
    if root_spec is None:
        # Shouldn't happen because root id was seeded into the queue.
        raise RuntimeError(f"Root workflow '{root_id}' was not compiled/registered.")
    return root_spec, registry


class WorkflowAgent(BaseAgent):
    """Run a VisualFlow workflow as an AbstractCode agent.

    Contract: the workflow must declare `interfaces: ["abstractcode.agent.v1"]` and expose:
    - On Flow Start output pin: `request` (string)
    - On Flow End input pin: `response` (string)
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

        resolved = resolve_visual_flow(self._flow_ref, flows_dir=flows_dir)
        self.visual_flow = resolved.visual_flow
        self.flows = resolved.flows
        self.flows_dir = resolved.flows_dir

        # Validate interface contract before creating the workflow spec.
        try:
            from abstractflow.visual.interfaces import (
                ABSTRACTCODE_AGENT_V1,
                apply_visual_flow_interface_scaffold,
                validate_visual_flow_interface,
            )
        except Exception as e:  # pragma: no cover
            raise RuntimeError(
                "AbstractFlow is required to validate VisualFlow interfaces.\n"
                'Install with: pip install "abstractcode[flow]"'
            ) from e

        # Authoring UX: keep interface-marked flows scaffolded even if the underlying
        # JSON was created before the contract expanded (or was edited manually).
        try:
            apply_visual_flow_interface_scaffold(self.visual_flow, ABSTRACTCODE_AGENT_V1, include_recommended=True)
        except Exception:
            pass

        errors = validate_visual_flow_interface(self.visual_flow, ABSTRACTCODE_AGENT_V1)
        if errors:
            joined = "\n".join([f"- {e}" for e in errors])
            raise ValueError(f"Workflow does not implement '{ABSTRACTCODE_AGENT_V1}':\n{joined}")

        self._last_task: Optional[str] = None

        super().__init__(
            runtime=runtime,
            tools=tools,
            on_step=on_step,
            actor_id=actor_id,
            session_id=session_id,
        )

    def _create_workflow(self) -> WorkflowSpec:
        tools = list(self.tools or [])
        spec, _registry = _compile_visual_flow_tree(root=self.visual_flow, flows=self.flows, tools=tools, runtime=self.runtime)
        return spec

    def start(self, task: str, *, allowed_tools: Optional[List[str]] = None, **_: Any) -> str:
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
            "request": task,
            "context": {"task": task, "messages": _copy_messages(self.session_messages)},
            "_temp": {},
            "_limits": limits,
        }

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

        run_id = self.runtime.start(
            workflow=self.workflow,
            vars=vars,
            actor_id=self._ensure_actor_id(),
            session_id=self._ensure_session_id(),
        )
        self._current_run_id = run_id

        if self.on_step:
            try:
                self.on_step(
                    "init",
                    {
                        "flow_id": str(getattr(self.visual_flow, "id", "") or ""),
                        "flow_name": str(getattr(self.visual_flow, "name", "") or ""),
                    },
                )
            except Exception:
                pass

        return run_id

    def step(self) -> RunState:
        if not self._current_run_id:
            raise RuntimeError("No active run. Call start() first.")

        state = self.runtime.tick(workflow=self.workflow, run_id=self._current_run_id, max_steps=1)

        if state.status == RunStatus.COMPLETED:
            response_text = ""
            out = getattr(state, "output", None)
            if isinstance(out, dict):
                if isinstance(out.get("response"), str):
                    response_text = str(out.get("response") or "")
                else:
                    result = out.get("result")
                    if isinstance(result, dict) and "response" in result:
                        response_text = str(result.get("response") or "")
                    elif isinstance(result, str):
                        response_text = str(result or "")

            task = str(self._last_task or "")
            ctx = state.vars.get("context") if isinstance(getattr(state, "vars", None), dict) else None
            if not isinstance(ctx, dict):
                ctx = {"task": task, "messages": []}
                state.vars["context"] = ctx

            msgs_raw = ctx.get("messages")
            msgs = _copy_messages(msgs_raw)
            msgs.append(_new_message(role="user", content=task))
            msgs.append(_new_message(role="assistant", content=response_text))
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
                    self.on_step("done", {"answer": response_text})
                except Exception:
                    pass

        if state.status in (RunStatus.FAILED, RunStatus.CANCELLED):
            self._sync_session_caches_from_state(state)

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
