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

_STATUS_EVENT_NAME = "abstractcode.status"
_MESSAGE_EVENT_NAME = "abstractcode.message"
_TOOL_EXEC_EVENT_NAME = "abstractcode.tool_execution"
_TOOL_RESULT_EVENT_NAME = "abstractcode.tool_result"


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
            for n in getattr(self.visual_flow, "nodes", []) or []:
                nid = getattr(n, "id", None)
                if not isinstance(nid, str) or not nid:
                    continue
                data = getattr(n, "data", None)
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
                        "flow_id": str(getattr(self.visual_flow, "id", "") or ""),
                        "flow_name": str(getattr(self.visual_flow, "name", "") or ""),
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

        if state.status == RunStatus.COMPLETED:
            response_text = ""
            meta_out: Dict[str, Any] = {}
            scratchpad_out: Any = None
            raw_result_out: Any = None
            out = getattr(state, "output", None)
            if isinstance(out, dict):
                result_payload = out.get("result") if isinstance(out.get("result"), dict) else None
                if isinstance(out.get("response"), str):
                    response_text = str(out.get("response") or "")
                else:
                    result = out.get("result")
                    if isinstance(result, dict) and "response" in result:
                        response_text = str(result.get("response") or "")
                    elif isinstance(result, str):
                        response_text = str(result or "")
                raw_meta = out.get("meta")
                if isinstance(raw_meta, dict):
                    meta_out = dict(raw_meta)
                elif isinstance(result_payload, dict) and isinstance(result_payload.get("meta"), dict):
                    meta_out = dict(result_payload.get("meta") or {})
                scratchpad_out = out.get("scratchpad")
                if scratchpad_out is None and isinstance(result_payload, dict) and "scratchpad" in result_payload:
                    scratchpad_out = result_payload.get("scratchpad")
                raw_result_out = out.get("raw_result")
                if raw_result_out is None and isinstance(result_payload, dict) and "raw_result" in result_payload:
                    raw_result_out = result_payload.get("raw_result")

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
            if raw_result_out is not None:
                assistant_meta["workflow_raw_result"] = raw_result_out

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
                            "meta": meta_out or None,
                            "scratchpad": scratchpad_out,
                            "raw_result": raw_result_out,
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
