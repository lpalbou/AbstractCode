from __future__ import annotations

import json
import os
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional, Tuple, Literal


FlowVerbosity = Literal["none", "default", "full"]


@dataclass
class FlowRunResult:
    """Summary of a flow execution for host-side UX and REPL context injection."""

    flow_id: str
    flow_name: str
    run_id: str
    status: str
    store_dir: Optional[str]
    tool_calls: List[Dict[str, Any]]


@dataclass(frozen=True)
class FlowRunRef:
    """Durable reference to a visual-flow run (the full state lives in the RunStore)."""

    flow_id: str
    flows_dir: str
    run_id: str


def default_flow_state_file() -> str:
    env = os.getenv("ABSTRACTCODE_FLOW_STATE_FILE")
    if env:
        return env
    return str(Path.home() / ".abstractcode" / "flow_state.json")


def default_flows_dir() -> Path:
    env = os.getenv("ABSTRACTFLOW_FLOWS_DIR")
    if env:
        return Path(env)
    # Monorepo-friendly default.
    candidate = Path("abstractflow/web/flows")
    if candidate.exists() and candidate.is_dir():
        return candidate
    return Path("flows")


def _read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def _write_json(path: Path, data: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, ensure_ascii=False), encoding="utf-8")


def _load_flow_ref(path: Path) -> Optional[FlowRunRef]:
    if not path.exists():
        return None
    try:
        raw = _read_json(path)
    except Exception:
        return None
    if not isinstance(raw, dict):
        return None
    if raw.get("kind") and raw.get("kind") != "flow":
        return None
    flow_id = raw.get("flow_id")
    flows_dir = raw.get("flows_dir")
    run_id = raw.get("run_id")
    if not isinstance(flow_id, str) or not flow_id.strip():
        return None
    if not isinstance(flows_dir, str) or not flows_dir.strip():
        return None
    if not isinstance(run_id, str) or not run_id.strip():
        return None
    return FlowRunRef(flow_id=flow_id.strip(), flows_dir=flows_dir.strip(), run_id=run_id.strip())


def _save_flow_ref(path: Path, ref: FlowRunRef) -> None:
    _write_json(
        path,
        {
            "kind": "flow",
            "flow_id": ref.flow_id,
            "flows_dir": ref.flows_dir,
            "run_id": ref.run_id,
        },
    )

def _flow_store_dir(state_path: Path) -> Path:
    return state_path.with_name(state_path.stem + ".d")


def _parse_input_json(*, raw_json: Optional[str], json_path: Optional[str]) -> Dict[str, Any]:
    if raw_json and json_path:
        raise ValueError("Provide either --input-json or --input-file, not both.")

    if json_path:
        payload = _read_json(Path(json_path).expanduser().resolve())
        if not isinstance(payload, dict):
            raise ValueError("--input-file must contain a JSON object")
        return dict(payload)

    if raw_json:
        payload = json.loads(raw_json)
        if not isinstance(payload, dict):
            raise ValueError("--input-json must be a JSON object")
        return dict(payload)

    return {}


def _coerce_value(raw: str) -> Any:
    v = str(raw)
    lower = v.strip().lower()
    if lower in ("true", "yes", "y"):
        return True
    if lower in ("false", "no", "n"):
        return False
    if lower in ("null", "none"):
        return None

    # JSON objects/arrays (useful for payload-like params).
    if v and v[0] in ("{", "["):
        try:
            return json.loads(v)
        except Exception:
            pass

    # Integers
    try:
        if lower.startswith(("+", "-")):
            int_candidate = lower[1:]
        else:
            int_candidate = lower
        if int_candidate.isdigit():
            return int(lower, 10)
    except Exception:
        pass

    # Floats
    try:
        if any(c in lower for c in (".", "e")):
            return float(lower)
    except Exception:
        pass

    return v


def _parse_kv_list(items: List[str]) -> Dict[str, Any]:
    out: Dict[str, Any] = {}
    for item in items:
        raw = str(item or "").strip()
        if not raw:
            continue
        if "=" not in raw:
            raise ValueError(f"Invalid --param value (expected key=value): {raw}")
        k, v = raw.split("=", 1)
        key = k.strip()
        if not key:
            raise ValueError(f"Invalid --param key: {raw}")
        out[key] = _coerce_value(v)
    return out


def _parse_unknown_params(argv: List[str]) -> Dict[str, Any]:
    """Parse unknown CLI args as input params.

    Supports:
    - --key=value
    - --key value
    - key=value
    - --flag   (sets flag=true)
    """
    out: Dict[str, Any] = {}
    i = 0
    while i < len(argv):
        token = str(argv[i] or "")
        if not token:
            i += 1
            continue

        if token.startswith("--"):
            keyval = token[2:]
            if not keyval:
                raise ValueError("Invalid parameter flag '--'")
            if "=" in keyval:
                k, v = keyval.split("=", 1)
                key = k.strip()
                if not key:
                    raise ValueError(f"Invalid parameter flag: {token}")
                out[key] = _coerce_value(v)
                i += 1
                continue

            key = keyval.strip()
            if not key:
                raise ValueError(f"Invalid parameter flag: {token}")
            if i + 1 < len(argv) and not str(argv[i + 1]).startswith("--"):
                nxt = str(argv[i + 1])
                # Heuristic: if the next token looks like a standalone `key=value`,
                # treat this flag as boolean and let the next token be parsed normally.
                if "=" in nxt:
                    out[key] = True
                    i += 1
                    continue
                out[key] = _coerce_value(nxt)
                i += 2
                continue
            out[key] = True
            i += 1
            continue

        if "=" in token:
            k, v = token.split("=", 1)
            key = k.strip()
            if not key:
                raise ValueError(f"Invalid parameter: {token}")
            out[key] = _coerce_value(v)
            i += 1
            continue

        raise ValueError(f"Unexpected argument: {token}")

    return out


def _required_entry_inputs(vf: Any) -> List[str]:
    """Return required entry-node input keys (entry outputs excluding execution).

    AbstractFlow's web UI uses the entry node's output pins as run inputs. For the
    CLI, we fail fast if any of these are missing instead of prompting.
    """

    try:
        nodes = getattr(vf, "nodes", None)
    except Exception:
        nodes = None
    if not isinstance(nodes, list) or not nodes:
        return []

    try:
        entry_id = getattr(vf, "entryNode", None)
    except Exception:
        entry_id = None

    ENTRY_TYPES = {"on_flow_start", "on_user_request", "on_agent_message", "on_schedule", "on_event"}

    def _node_type(node: Any) -> str:
        t = getattr(node, "type", None)
        return t.value if hasattr(t, "value") else str(t or "")

    entry = None
    if isinstance(entry_id, str) and entry_id:
        for n in nodes:
            if str(getattr(n, "id", "") or "") == entry_id:
                entry = n
                break
    if entry is None:
        for n in nodes:
            if _node_type(n) in ENTRY_TYPES:
                entry = n
                break
    if entry is None:
        entry = nodes[0]

    data = getattr(entry, "data", None)
    raw_pins = None
    if isinstance(data, dict):
        raw_pins = data.get("outputs")
    if raw_pins is None:
        raw_pins = getattr(entry, "outputs", None)

    required: List[str] = []
    if isinstance(raw_pins, list):
        for p in raw_pins:
            if isinstance(p, dict):
                pid = str(p.get("id") or "").strip()
                ptype = str(p.get("type") or "").strip()
            else:
                pid = str(getattr(p, "id", "") or "").strip()
                ptype = str(getattr(getattr(p, "type", None), "value", None) or getattr(p, "type", "") or "").strip()
            if not pid or ptype == "execution":
                continue
            required.append(pid)

    return required


def _render_text(text: str) -> str:
    """Render UI-facing text without showing escaped newlines."""
    s = str(text)
    if "\\n" in s or "\\t" in s:
        s = s.replace("\\n", "\n").replace("\\t", "\t")
    return s


def _load_visual_flows(flows_dir: Path) -> Dict[str, Any]:
    try:
        from abstractflow.visual.models import VisualFlow
    except Exception as e:
        raise RuntimeError(
            "AbstractFlow is required to run VisualFlow workflows.\n"
            "Install with: pip install \"abstractcode[flow]\""
        ) from e

    flows: Dict[str, Any] = {}
    if not flows_dir.exists():
        return flows
    for path in sorted(flows_dir.glob("*.json")):
        try:
            raw = path.read_text(encoding="utf-8")
        except Exception:
            continue
        try:
            vf = VisualFlow.model_validate_json(raw)
        except Exception:
            continue
        flows[str(vf.id)] = vf
    return flows


def _resolve_flow(
    flow_ref: str,
    *,
    flows_dir: Optional[str],
) -> Tuple[Any, Dict[str, Any], Path]:
    """Resolve a VisualFlow either by id (in flows_dir) or by a .json path."""
    ref = str(flow_ref or "").strip()
    if not ref:
        raise ValueError("flow reference is required (flow id or .json path)")

    path = Path(ref).expanduser()
    flows_dir_path: Path
    if path.exists() and path.is_file():
        try:
            raw = path.read_text(encoding="utf-8")
        except Exception as e:
            raise ValueError(f"Cannot read flow file: {path}") from e

        try:
            from abstractflow.visual.models import VisualFlow
        except Exception as e:
            raise RuntimeError(
                "AbstractFlow is required to run VisualFlow workflows.\n"
                "Install with: pip install \"abstractcode[flow]\""
            ) from e

        vf = VisualFlow.model_validate_json(raw)
        flows_dir_path = Path(flows_dir).expanduser().resolve() if flows_dir else path.parent.resolve()
        flows = _load_visual_flows(flows_dir_path)
        flows[str(vf.id)] = vf
        return vf, flows, flows_dir_path

    flows_dir_path = Path(flows_dir).expanduser().resolve() if flows_dir else default_flows_dir().resolve()
    flows = _load_visual_flows(flows_dir_path)
    if ref not in flows:
        raise ValueError(f"Flow '{ref}' not found in {flows_dir_path}")
    return flows[ref], flows, flows_dir_path


def _is_pause_wait(wait: Any, *, run_id: str) -> bool:
    wait_key = getattr(wait, "wait_key", None)
    if isinstance(wait_key, str) and wait_key == f"pause:{run_id}":
        return True
    details = getattr(wait, "details", None)
    if isinstance(details, dict) and details.get("kind") == "pause":
        return True
    return False


def _extract_sub_run_id(wait: Any) -> Optional[str]:
    details = getattr(wait, "details", None)
    if isinstance(details, dict):
        sub_run_id = details.get("sub_run_id")
        if isinstance(sub_run_id, str) and sub_run_id:
            return sub_run_id
    wait_key = getattr(wait, "wait_key", None)
    if isinstance(wait_key, str) and wait_key.startswith("subworkflow:"):
        return wait_key.split("subworkflow:", 1)[1] or None
    return None


def _iter_descendants(runtime: Any, root_run_id: str) -> List[str]:
    """Return [root] + descendants (best-effort) using QueryableRunStore.list_children."""
    out: List[str] = [root_run_id]
    seen = {root_run_id}
    queue = [root_run_id]

    run_store = getattr(runtime, "run_store", None)
    list_children = getattr(run_store, "list_children", None)
    if not callable(list_children):
        return out

    while queue:
        current = queue.pop(0)
        try:
            children = list_children(parent_run_id=current)  # type: ignore[misc]
        except Exception:
            continue
        if not isinstance(children, list):
            continue
        for child in children:
            cid = getattr(child, "run_id", None)
            if not isinstance(cid, str) or not cid or cid in seen:
                continue
            seen.add(cid)
            out.append(cid)
            queue.append(cid)
    return out


def _workflow_for(runtime: Any, runner_workflow: Any, workflow_id: str) -> Any:
    reg = getattr(runtime, "workflow_registry", None)
    if reg is not None:
        getter = getattr(reg, "get", None)
        if callable(getter):
            wf = getter(workflow_id)
            if wf is not None:
                return wf
    if getattr(runner_workflow, "workflow_id", None) == workflow_id:
        return runner_workflow
    raise RuntimeError(f"Workflow '{workflow_id}' not found in runtime registry")


def _print_answer_user_records(
    *,
    runtime: Any,
    run_ids: Iterable[str],
    offsets: Dict[str, int],
    emit: Any,
) -> None:
    for rid in run_ids:
        ledger = runtime.get_ledger(rid)
        if not isinstance(ledger, list):
            continue
        start = int(offsets.get(rid, 0) or 0)
        if start < 0:
            start = 0
        for rec in ledger[start:]:
            if not isinstance(rec, dict):
                continue
            if rec.get("status") != "completed":
                continue
            eff = rec.get("effect")
            if not isinstance(eff, dict):
                continue
            if eff.get("type") != "answer_user":
                continue
            result = rec.get("result")
            if isinstance(result, dict) and isinstance(result.get("message"), str):
                emit(_render_text(result["message"]))
        offsets[rid] = len(ledger)


@dataclass
class _ApprovalState:
    approve_all: bool = False


def _approve_and_execute(
    *,
    tool_calls: List[Dict[str, Any]],
    tool_runner: Any,
    auto_approve: bool,
    approval_state: _ApprovalState,
    prompt_fn: Any,
    print_fn: Any,
    trace: Optional[FlowRunResult] = None,
) -> Optional[Dict[str, Any]]:
    if auto_approve or approval_state.approve_all:
        payload = tool_runner.execute(tool_calls=tool_calls)
        if trace is not None and isinstance(payload, dict):
            _capture_tool_results(trace=trace, tool_calls=tool_calls, payload=payload)
        return payload

    print_fn("\nTool approval required")
    print_fn("-" * 60)
    approve_all = False
    approved: List[Dict[str, Any]] = []
    results: List[Dict[str, Any]] = []

    for tc in tool_calls:
        name = str(tc.get("name", "") or "")
        args = dict(tc.get("arguments") or {})
        call_id = str(tc.get("call_id") or "")

        print_fn(f"\n{name}")
        print_fn("args: " + json.dumps(args, indent=2, ensure_ascii=False))

        if not approve_all:
            while True:
                choice = str(prompt_fn("Approve? [y]es/[n]o/[a]ll/[q]uit: ")).strip().lower()
                if choice in ("y", "yes"):
                    break
                if choice in ("a", "all"):
                    approve_all = True
                    approval_state.approve_all = True
                    break
                if choice in ("n", "no"):
                    results.append(
                        {"call_id": call_id, "name": name, "success": False, "output": None, "error": "Rejected by user"}
                    )
                    name = ""
                    break
                if choice in ("q", "quit"):
                    return None
                print_fn("Invalid choice.")

        if not name:
            continue
        approved.append({"name": name, "arguments": args, "call_id": call_id})

    if approved:
        payload = tool_runner.execute(tool_calls=approved)
        if trace is not None and isinstance(payload, dict):
            _capture_tool_results(trace=trace, tool_calls=approved, payload=payload)
        if isinstance(payload, dict):
            exec_results = payload.get("results")
            if isinstance(exec_results, list):
                results.extend(exec_results)
        else:
            results.append({"call_id": "", "name": "tools", "success": False, "output": None, "error": "Invalid tool runner output"})

    return {"mode": "executed", "results": results}


def _capture_tool_results(*, trace: FlowRunResult, tool_calls: List[Dict[str, Any]], payload: Dict[str, Any]) -> None:
    """Capture executed tool call metadata into the flow result (no truncation)."""
    results = payload.get("results")
    results_by_id: Dict[str, Dict[str, Any]] = {}
    if isinstance(results, list):
        for r in results:
            if not isinstance(r, dict):
                continue
            cid = str(r.get("call_id") or "")
            if cid:
                results_by_id[cid] = r

    for tc in tool_calls:
        if not isinstance(tc, dict):
            continue
        call_id = str(tc.get("call_id") or "")
        name = str(tc.get("name") or "")
        args = tc.get("arguments")
        args_dict = dict(args) if isinstance(args, dict) else {}
        r = results_by_id.get(call_id, {})
        trace.tool_calls.append(
            {
                "name": name,
                "arguments": args_dict,
                "success": bool(r.get("success")) if isinstance(r, dict) and "success" in r else None,
                "error": r.get("error") if isinstance(r, dict) else None,
                "output_chars": len(str(r.get("output") or "")) if isinstance(r, dict) else None,
            }
        )


def _node_meta(vf: Any) -> Dict[str, Dict[str, str]]:
    meta: Dict[str, Dict[str, str]] = {}
    nodes = getattr(vf, "nodes", None)
    if not isinstance(nodes, list):
        return meta
    for n in nodes:
        try:
            node_id = str(getattr(n, "id", "") or "")
            if not node_id:
                continue
            node_type = getattr(n, "type", None)
            node_type_val = getattr(node_type, "value", None)
            ntype = str(node_type_val if node_type_val is not None else node_type or "")
            label = ""
            data = getattr(n, "data", None)
            if isinstance(data, dict):
                label = str(data.get("label") or "")
            if not label:
                label = str(getattr(n, "label", "") or "")
            if not label:
                label = ntype or node_id
            meta[node_id] = {"label": label, "type": ntype}
        except Exception:
            continue
    return meta


def _duration_ms(rec: Dict[str, Any]) -> Optional[float]:
    started = rec.get("started_at")
    ended = rec.get("ended_at")
    if not isinstance(started, str) or not isinstance(ended, str) or not started or not ended:
        return None
    try:
        from datetime import datetime

        s = datetime.fromisoformat(started.replace("Z", "+00:00"))
        e = datetime.fromisoformat(ended.replace("Z", "+00:00"))
        return float((e - s).total_seconds() * 1000.0)
    except Exception:
        return None


def _print_step_records(
    *,
    runtime: Any,
    run_ids: Iterable[str],
    offsets: Dict[str, int],
    emit: Any,
    verbosity: FlowVerbosity,
    node_meta: Dict[str, Dict[str, str]],
) -> None:
    """Print new ledger records for observability (like AbstractFlow left panel)."""
    if verbosity == "none":
        return

    for rid in run_ids:
        ledger = runtime.get_ledger(rid)
        if not isinstance(ledger, list):
            continue
        start = int(offsets.get(rid, 0) or 0)
        if start < 0:
            start = 0

        for rec in ledger[start:]:
            if not isinstance(rec, dict):
                continue
            status = rec.get("status")
            eff = rec.get("effect")
            eff_type = None
            if isinstance(eff, dict):
                eff_type = eff.get("type")
            if verbosity != "full" and eff_type == "answer_user":
                # answer_user is printed separately (as the user-visible output).
                continue

            nid = str(rec.get("node_id") or "")
            meta = node_meta.get(nid, {})
            label = meta.get("label") or nid
            ntype = meta.get("type") or ""

            if verbosity == "full":
                emit(json.dumps(rec, indent=2, ensure_ascii=False))
                continue

            dur = _duration_ms(rec)
            dur_txt = f"{dur/1000.0:.2f}s" if isinstance(dur, (int, float)) else ""
            parts = [f"{label}"]
            if ntype:
                parts.append(f"({ntype})")
            if isinstance(eff_type, str) and eff_type:
                parts.append(str(eff_type))
            if isinstance(status, str) and status:
                parts.append(str(status).upper())
            if dur_txt:
                parts.append(dur_txt)
            emit(" ".join([p for p in parts if p]))

        offsets[rid] = len(ledger)


def _resume_and_bubble(
    *,
    runtime: Any,
    runner_workflow: Any,
    top_run_id: str,
    target_run_id: str,
    payload: Dict[str, Any],
    wait_key: Optional[str],
) -> None:
    """Resume `target_run_id` and bubble subworkflow completions up to `top_run_id`."""
    from abstractruntime.core.models import RunStatus, WaitReason

    def _spec_for(state: Any) -> Any:
        return _workflow_for(runtime, runner_workflow, getattr(state, "workflow_id", ""))

    target_state = runtime.get_state(target_run_id)
    runtime.resume(
        workflow=_spec_for(target_state),
        run_id=target_run_id,
        wait_key=wait_key,
        payload=payload,
        max_steps=0,
    )

    current_run_id = target_run_id
    for _ in range(50):
        st = runtime.get_state(current_run_id)
        if st.status == RunStatus.RUNNING:
            st = runtime.tick(workflow=_spec_for(st), run_id=current_run_id, max_steps=100)

        if st.status == RunStatus.WAITING:
            return
        if st.status == RunStatus.FAILED:
            raise RuntimeError(st.error or "Subworkflow failed")
        if st.status != RunStatus.COMPLETED:
            return

        parent_id = getattr(st, "parent_run_id", None)
        if not isinstance(parent_id, str) or not parent_id:
            return

        parent = runtime.get_state(parent_id)
        if parent.status != RunStatus.WAITING or parent.waiting is None:
            return
        if parent.waiting.reason != WaitReason.SUBWORKFLOW:
            return

        runtime.resume(
            workflow=_spec_for(parent),
            run_id=parent_id,
            wait_key=None,
            payload={
                "sub_run_id": st.run_id,
                "output": st.output,
                "node_traces": runtime.get_node_traces(st.run_id),
            },
            max_steps=0,
        )

        if parent_id == top_run_id:
            return
        current_run_id = parent_id


def _drive_until_blocked(
    *,
    runner: Any,
    tool_runner: Any,
    auto_approve: bool,
    wait_until: bool,
    verbosity: FlowVerbosity = "default",
    node_meta: Optional[Dict[str, Dict[str, str]]] = None,
    trace: Optional[FlowRunResult] = None,
    prompt_fn: Any = None,
    ask_user_fn: Any = None,
    print_fn: Any = None,
    approval_state: Optional[_ApprovalState] = None,
    on_answer_user: Any = None,
) -> None:
    """Drive a visual-flow session until completion or an external wait."""
    from abstractruntime.core.models import RunStatus, WaitReason

    runtime = runner.runtime
    top_run_id = runner.run_id
    if not isinstance(top_run_id, str) or not top_run_id:
        raise RuntimeError("Runner has no run_id")

    answer_offsets: Dict[str, int] = {}
    step_offsets: Dict[str, int] = {}
    approval = approval_state or _ApprovalState()
    meta = node_meta or {}

    _print = print_fn or print
    _prompt = prompt_fn or (lambda msg: input(msg))
    def _default_ask_user(prompt: str, choices: Optional[List[str]]) -> Optional[str]:
        if isinstance(choices, list) and choices:
            for i, c in enumerate(choices):
                _print(f"[{i+1}] {c}")
        return input(prompt + " ").strip()

    _ask_user = ask_user_fn or _default_ask_user
    _emit_answer = on_answer_user or _print

    def _tick_ready_runs(run_ids: List[str]) -> None:
        for rid in run_ids:
            st = runtime.get_state(rid)
            if st.status == RunStatus.RUNNING:
                wf = _workflow_for(runtime, runner.workflow, st.workflow_id)
                runtime.tick(workflow=wf, run_id=rid, max_steps=10)
                continue
            if st.status == RunStatus.WAITING and st.waiting and st.waiting.reason == WaitReason.UNTIL:
                wf = _workflow_for(runtime, runner.workflow, st.workflow_id)
                runtime.tick(workflow=wf, run_id=rid, max_steps=10)

    while True:
        run_ids = _iter_descendants(runtime, top_run_id)
        _tick_ready_runs(run_ids)
        _print_step_records(runtime=runtime, run_ids=run_ids, offsets=step_offsets, emit=_print, verbosity=verbosity, node_meta=meta)
        _print_answer_user_records(runtime=runtime, run_ids=run_ids, offsets=answer_offsets, emit=_emit_answer)

        top = runtime.get_state(top_run_id)
        if top.status == RunStatus.COMPLETED:
            # Mirror VisualSessionRunner semantics: finish when children are idle listeners or terminal.
            all_idle_or_done = True
            for rid in run_ids:
                if rid == top_run_id:
                    continue
                st = runtime.get_state(rid)
                if st.status in (RunStatus.COMPLETED, RunStatus.FAILED, RunStatus.CANCELLED):
                    continue
                if st.status == RunStatus.WAITING and st.waiting and st.waiting.reason == WaitReason.EVENT:
                    continue
                all_idle_or_done = False
            if all_idle_or_done:
                # Cancel idle listeners so the session ends cleanly.
                for rid in run_ids:
                    if rid == top_run_id:
                        continue
                    st = runtime.get_state(rid)
                    if st.status == RunStatus.WAITING and st.waiting and st.waiting.reason == WaitReason.EVENT:
                        try:
                            runtime.cancel_run(rid, reason="Session completed")
                        except Exception:
                            pass
                return
            continue

        if top.status == RunStatus.FAILED:
            raise RuntimeError(top.error or "Flow failed")
        if top.status == RunStatus.CANCELLED:
            print("Run cancelled.")
            return

        if top.status != RunStatus.WAITING or top.waiting is None:
            continue

        # Resolve deepest waiting run for subworkflow chains.
        target_run_id = top_run_id
        while True:
            st = runtime.get_state(target_run_id)
            if st.status != RunStatus.WAITING or st.waiting is None:
                break
            if st.waiting.reason != WaitReason.SUBWORKFLOW:
                break
            nxt = _extract_sub_run_id(st.waiting)
            if not nxt:
                break
            target_run_id = nxt

        target = runtime.get_state(target_run_id)
        wait = target.waiting
        if wait is None:
            continue

        # Paused runs should be resumed via Runtime.resume_run().
        if wait.reason == WaitReason.USER and _is_pause_wait(wait, run_id=target_run_id):
            print(f"Run is paused ({target_run_id}). Use `abstractcode flow resume-run` to continue.")
            return

        if wait.reason == WaitReason.USER:
            prompt = _render_text(getattr(wait, "prompt", None) or "Please respond:")
            choices = getattr(wait, "choices", None)
            if not isinstance(choices, list):
                choices = None
            response = _ask_user(prompt, choices)
            if response is None:
                _print("Left run waiting (not resumed).")
                return
            response = str(response).strip()
            _resume_and_bubble(
                runtime=runtime,
                runner_workflow=runner.workflow,
                top_run_id=top_run_id,
                target_run_id=target_run_id,
                payload={"response": response},
                wait_key=getattr(wait, "wait_key", None),
            )
            continue

        if wait.reason == WaitReason.EVENT:
            details = getattr(wait, "details", None)
            tool_calls = details.get("tool_calls") if isinstance(details, dict) else None
            if isinstance(tool_calls, list):
                payload = _approve_and_execute(
                    tool_calls=tool_calls,
                    tool_runner=tool_runner,
                    auto_approve=auto_approve,
                    approval_state=approval,
                    prompt_fn=_prompt,
                    print_fn=_print,
                    trace=trace,
                )
                if payload is None:
                    _print("Left run waiting (not resumed).")
                    return
                _resume_and_bubble(
                    runtime=runtime,
                    runner_workflow=runner.workflow,
                    top_run_id=top_run_id,
                    target_run_id=target_run_id,
                    payload=payload,
                    wait_key=getattr(wait, "wait_key", None),
                )
                continue

            # Event waits can carry prompt/choices for durable human input.
            prompt = getattr(wait, "prompt", None)
            if isinstance(prompt, str) and prompt.strip():
                choices = getattr(wait, "choices", None)
                if not isinstance(choices, list):
                    choices = None
                response = _ask_user(_render_text(prompt), choices)
                if response is None:
                    _print("Left run waiting (not resumed).")
                    return
                response = str(response).strip()
                _resume_and_bubble(
                    runtime=runtime,
                    runner_workflow=runner.workflow,
                    top_run_id=top_run_id,
                    target_run_id=target_run_id,
                    payload={"response": response},
                    wait_key=getattr(wait, "wait_key", None),
                )
                continue

            _print(f"Waiting for event: {getattr(wait, 'wait_key', None)}")
            return

        if wait.reason == WaitReason.UNTIL:
            until = getattr(wait, "until", None)
            _print(f"Waiting until: {until}")
            if not wait_until or not isinstance(until, str) or not until:
                return

            # Sleep in coarse increments to keep the CLI responsive.
            try:
                import datetime as _dt

                u = until
                if u.endswith("Z"):
                    u = u[:-1] + "+00:00"
                due = _dt.datetime.fromisoformat(u)
                now = _dt.datetime.now(_dt.timezone.utc)
                delta_s = max(0.0, (due - now).total_seconds())
            except Exception:
                delta_s = 1.0
            time.sleep(min(delta_s, 60.0))
            continue

        if wait.reason == WaitReason.SUBWORKFLOW:
            _print("Waiting for subworkflow…")
            return

        _print(f"Waiting: {wait.reason.value} ({getattr(wait, 'wait_key', None)})")
        return


def run_flow_command(
    *,
    flow_ref: str,
    flows_dir: Optional[str],
    input_json: Optional[str],
    input_file: Optional[str],
    params: List[str],
    extra_args: List[str],
    flow_state_file: Optional[str],
    no_state: bool,
    auto_approve: bool,
    wait_until: bool,
    verbosity: FlowVerbosity = "default",
    prompt_fn: Any = None,
    ask_user_fn: Any = None,
    print_fn: Any = None,
    on_answer_user: Any = None,
) -> FlowRunResult:
    try:
        import abstractflow  # noqa: F401
    except Exception as e:
        raise RuntimeError(
            "AbstractFlow is required to run VisualFlow workflows.\n"
            "Install with: pip install \"abstractcode[flow]\""
        ) from e

    from abstractruntime.integrations.abstractcore import MappingToolExecutor, PassthroughToolExecutor
    from abstractruntime.integrations.abstractcore.default_tools import get_default_tools
    from abstractruntime.storage.artifacts import FileArtifactStore, InMemoryArtifactStore
    from abstractruntime.storage.in_memory import InMemoryLedgerStore, InMemoryRunStore
    from abstractruntime.storage.json_files import JsonFileRunStore, JsonlLedgerStore

    vf, flows, flows_dir_path = _resolve_flow(flow_ref, flows_dir=flows_dir)
    input_data = _parse_input_json(raw_json=input_json, json_path=input_file)
    input_data.update(_parse_kv_list(params))
    input_data.update(_parse_unknown_params(extra_args))

    # Fail fast if the flow declares entry inputs and the user didn't provide them.
    required = _required_entry_inputs(vf)
    missing = [k for k in required if k not in input_data]
    if missing:
        missing_txt = ", ".join(missing)
        raise ValueError(
            f"Missing required flow inputs: {missing_txt}. "
            f"Provide them as flags (e.g. --{missing[0]} ...) or via --input-json/--input-file/--param."
        )

    # Stores: file-backed only when state is enabled.
    state_path = Path(flow_state_file or default_flow_state_file()).expanduser().resolve()
    if no_state:
        run_store = InMemoryRunStore()
        ledger_store = InMemoryLedgerStore()
        artifact_store = InMemoryArtifactStore()
    else:
        state_path.parent.mkdir(parents=True, exist_ok=True)
        store_dir = _flow_store_dir(state_path)
        run_store = JsonFileRunStore(store_dir)
        ledger_store = JsonlLedgerStore(store_dir)
        artifact_store = FileArtifactStore(store_dir)

    tool_executor = PassthroughToolExecutor(mode="approval_required")
    tool_runner = MappingToolExecutor.from_tools(get_default_tools())

    from abstractflow.visual.executor import create_visual_runner

    runner = create_visual_runner(
        vf,
        flows=flows,
        run_store=run_store,
        ledger_store=ledger_store,
        artifact_store=artifact_store,
        tool_executor=tool_executor,
    )

    run_id = runner.start(input_data)
    state_path = Path(flow_state_file or default_flow_state_file()).expanduser().resolve()
    store_dir: Optional[str] = None
    if not no_state:
        store_dir = str(_flow_store_dir(state_path))
    trace = FlowRunResult(
        flow_id=str(vf.id),
        flow_name=str(getattr(vf, "name", "") or str(vf.id)),
        run_id=str(run_id),
        status="running",
        store_dir=store_dir,
        tool_calls=[],
    )

    if not no_state:
        _save_flow_ref(state_path, FlowRunRef(flow_id=str(vf.id), flows_dir=str(flows_dir_path), run_id=run_id))

    try:
        _drive_until_blocked(
            runner=runner,
            tool_runner=tool_runner,
            auto_approve=auto_approve,
            wait_until=wait_until,
            verbosity=verbosity,
            node_meta=_node_meta(vf),
            trace=trace,
            prompt_fn=prompt_fn,
            ask_user_fn=ask_user_fn,
            print_fn=print_fn,
            on_answer_user=on_answer_user,
        )
        try:
            trace.status = str(runner.runtime.get_state(run_id).status.value)
        except Exception:
            trace.status = "unknown"
        return trace
    except KeyboardInterrupt:
        # Best-effort: pause the whole run tree so schedulers/event emitters won't advance it.
        try:
            for rid in _iter_descendants(runner.runtime, run_id):
                runner.runtime.pause_run(rid, reason="Paused via AbstractCode (KeyboardInterrupt)")
        except Exception:
            pass
        print("\nInterrupted. Run paused (best-effort).")
        try:
            trace.status = str(runner.runtime.get_state(run_id).status.value)
        except Exception:
            trace.status = "unknown"
        return trace


def resume_flow_command(
    *,
    flow_state_file: Optional[str],
    no_state: bool,
    auto_approve: bool,
    wait_until: bool,
    verbosity: FlowVerbosity = "default",
    prompt_fn: Any = None,
    ask_user_fn: Any = None,
    print_fn: Any = None,
    on_answer_user: Any = None,
) -> FlowRunResult:
    try:
        import abstractflow  # noqa: F401
    except Exception as e:
        raise RuntimeError(
            "AbstractFlow is required to run VisualFlow workflows.\n"
            "Install with: pip install \"abstractcode[flow]\""
        ) from e

    if no_state:
        raise ValueError("Cannot resume flows with --no-state (in-memory only).")

    state_path = Path(flow_state_file or default_flow_state_file()).expanduser().resolve()
    ref = _load_flow_ref(state_path)
    if ref is None:
        raise ValueError(f"No saved flow run found at {state_path}")

    flows_dir_path = Path(ref.flows_dir).expanduser().resolve()
    flows = _load_visual_flows(flows_dir_path)
    if ref.flow_id not in flows:
        raise ValueError(f"Flow '{ref.flow_id}' not found in {flows_dir_path}")
    vf = flows[ref.flow_id]

    from abstractruntime.integrations.abstractcore import MappingToolExecutor, PassthroughToolExecutor
    from abstractruntime.integrations.abstractcore.default_tools import get_default_tools
    from abstractruntime.storage.artifacts import FileArtifactStore
    from abstractruntime.storage.json_files import JsonFileRunStore, JsonlLedgerStore

    store_dir = _flow_store_dir(state_path)
    run_store = JsonFileRunStore(store_dir)
    ledger_store = JsonlLedgerStore(store_dir)
    artifact_store = FileArtifactStore(store_dir)

    tool_executor = PassthroughToolExecutor(mode="approval_required")
    tool_runner = MappingToolExecutor.from_tools(get_default_tools())

    from abstractflow.visual.executor import create_visual_runner

    runner = create_visual_runner(
        vf,
        flows=flows,
        run_store=run_store,
        ledger_store=ledger_store,
        artifact_store=artifact_store,
        tool_executor=tool_executor,
    )

    # Attach to existing run id.
    runner._current_run_id = ref.run_id  # type: ignore[attr-defined]
    trace = FlowRunResult(
        flow_id=str(vf.id),
        flow_name=str(getattr(vf, "name", "") or str(vf.id)),
        run_id=str(ref.run_id),
        status="running",
        store_dir=str(store_dir),
        tool_calls=[],
    )

    # Best-effort: if the run was paused, unpause it before continuing.
    try:
        for rid in _iter_descendants(runner.runtime, ref.run_id):
            runner.runtime.resume_run(rid)
    except Exception:
        pass

    _drive_until_blocked(
        runner=runner,
        tool_runner=tool_runner,
        auto_approve=auto_approve,
        wait_until=wait_until,
        verbosity=verbosity,
        node_meta=_node_meta(vf),
        trace=trace,
        prompt_fn=prompt_fn,
        ask_user_fn=ask_user_fn,
        print_fn=print_fn,
        on_answer_user=on_answer_user,
    )
    try:
        trace.status = str(runner.runtime.get_state(ref.run_id).status.value)
    except Exception:
        trace.status = "unknown"
    return trace


def control_flow_command(
    *,
    action: str,
    flow_state_file: Optional[str],
) -> None:
    """Pause/resume-run/cancel the current flow run (best-effort includes descendants)."""
    state_path = Path(flow_state_file or default_flow_state_file()).expanduser().resolve()
    ref = _load_flow_ref(state_path)
    if ref is None:
        raise ValueError(f"No saved flow run found at {state_path}")

    store_dir = _flow_store_dir(state_path)
    from abstractruntime.storage.json_files import JsonFileRunStore, JsonlLedgerStore
    from abstractruntime.storage.artifacts import FileArtifactStore
    from abstractruntime import Runtime

    run_store = JsonFileRunStore(store_dir)
    ledger_store = JsonlLedgerStore(store_dir)
    artifact_store = FileArtifactStore(store_dir)

    runtime = Runtime(run_store=run_store, ledger_store=ledger_store, artifact_store=artifact_store)
    run_ids = _iter_descendants(runtime, ref.run_id)

    action2 = str(action or "").strip().lower()
    if action2 == "pause":
        for rid in run_ids:
            runtime.pause_run(rid, reason="Paused via AbstractCode")
        print(f"Paused {len(run_ids)} run(s).")
        return
    if action2 == "resume":
        for rid in run_ids:
            runtime.resume_run(rid)
        print(f"Resumed {len(run_ids)} run(s).")
        return
    if action2 == "cancel":
        for rid in run_ids:
            runtime.cancel_run(rid, reason="Cancelled via AbstractCode")
        print(f"Cancelled {len(run_ids)} run(s).")
        return

    raise ValueError(f"Unknown control action: {action2}")


def list_flow_runs_command(
    *,
    flow_state_file: Optional[str],
    limit: int = 20,
) -> None:
    """List recent flow runs from the configured flow store directory."""
    state_path = Path(flow_state_file or default_flow_state_file()).expanduser().resolve()
    store_dir = _flow_store_dir(state_path)

    if not store_dir.exists():
        print(f"No flow run store found at {store_dir}")
        return

    from abstractruntime.core.models import RunStatus, WaitReason
    from abstractruntime.storage.json_files import JsonFileRunStore

    current = _load_flow_ref(state_path)
    current_run_id = current.run_id if current else None

    run_store = JsonFileRunStore(store_dir)
    runs = run_store.list_runs(limit=int(limit or 20))
    if not runs:
        print("No runs found.")
        return

    print(f"Store: {store_dir}")
    print("Most recent runs:")
    for r in runs:
        marker = "*" if current_run_id and r.run_id == current_run_id else " "
        wait = r.waiting
        wait_txt = ""
        if r.status == RunStatus.WAITING and wait is not None:
            reason = wait.reason.value if isinstance(wait.reason, WaitReason) else str(wait.reason)
            wait_txt = f" waiting={reason} key={wait.wait_key}"
        updated = r.updated_at or r.created_at or ""
        print(f"{marker} {r.run_id}  wf={r.workflow_id}  {r.status.value}  {updated}{wait_txt}")


def attach_flow_run_command(
    *,
    run_id: str,
    flows_dir: Optional[str],
    flow_state_file: Optional[str],
) -> None:
    """Set the current flow run reference to an existing run_id."""
    run_id2 = str(run_id or "").strip()
    if not run_id2:
        raise ValueError("run_id is required")

    state_path = Path(flow_state_file or default_flow_state_file()).expanduser().resolve()
    store_dir = _flow_store_dir(state_path)
    if not store_dir.exists():
        raise ValueError(f"No flow run store found at {store_dir}")

    from abstractruntime.storage.json_files import JsonFileRunStore

    run_store = JsonFileRunStore(store_dir)
    run = run_store.load(run_id2)
    if run is None:
        raise ValueError(f"Run not found: {run_id2}")

    flows_dir_path = Path(flows_dir).expanduser().resolve() if flows_dir else None
    current = _load_flow_ref(state_path)
    if flows_dir_path is None:
        if current and current.flows_dir:
            flows_dir_path = Path(current.flows_dir).expanduser().resolve()
        else:
            flows_dir_path = default_flows_dir().resolve()

    flows = _load_visual_flows(flows_dir_path)

    flow_id = None
    if isinstance(run.workflow_id, str) and run.workflow_id in flows:
        flow_id = run.workflow_id
    elif current and current.flow_id in flows:
        flow_id = current.flow_id

    if not flow_id:
        raise ValueError(
            f"Cannot infer flow id for run '{run_id2}' (workflow_id='{run.workflow_id}'). "
            "Provide --flows-dir pointing at the VisualFlow JSON directory."
        )

    _save_flow_ref(state_path, FlowRunRef(flow_id=str(flow_id), flows_dir=str(flows_dir_path), run_id=run_id2))
    print(f"Attached flow run: {run_id2} (flow={flow_id})")


def emit_flow_event_command(
    *,
    name: Optional[str],
    wait_key: Optional[str],
    scope: str,
    payload_json: Optional[str],
    payload_file: Optional[str],
    session_id: Optional[str],
    max_steps: int,
    flows_dir: Optional[str],
    flow_state_file: Optional[str],
    auto_approve: bool,
) -> None:
    """Emit a custom event (name/scope) or resume a raw wait_key."""
    if bool(name) == bool(wait_key):
        raise ValueError("Provide exactly one of --name or --wait-key")

    state_path = Path(flow_state_file or default_flow_state_file()).expanduser().resolve()
    ref = _load_flow_ref(state_path)
    if ref is None:
        raise ValueError(f"No saved flow run found at {state_path}")

    flows_dir_path = Path(flows_dir).expanduser().resolve() if flows_dir else Path(ref.flows_dir).expanduser().resolve()
    flows = _load_visual_flows(flows_dir_path)
    if ref.flow_id not in flows:
        raise ValueError(f"Flow '{ref.flow_id}' not found in {flows_dir_path}")
    vf = flows[ref.flow_id]

    def _load_payload() -> Dict[str, Any]:
        if payload_json and payload_file:
            raise ValueError("Provide either --payload-json or --payload-file, not both.")
        if payload_file:
            raw = _read_json(Path(payload_file).expanduser().resolve())
            if isinstance(raw, dict):
                return dict(raw)
            return {"value": raw}
        if payload_json:
            raw = json.loads(payload_json)
            if isinstance(raw, dict):
                return dict(raw)
            return {"value": raw}
        return {}

    payload = _load_payload()

    from abstractruntime.integrations.abstractcore import MappingToolExecutor, PassthroughToolExecutor
    from abstractruntime.integrations.abstractcore.default_tools import get_default_tools
    from abstractruntime.storage.artifacts import FileArtifactStore
    from abstractruntime.storage.json_files import JsonFileRunStore, JsonlLedgerStore

    store_dir = _flow_store_dir(state_path)
    run_store = JsonFileRunStore(store_dir)
    ledger_store = JsonlLedgerStore(store_dir)
    artifact_store = FileArtifactStore(store_dir)

    tool_executor = PassthroughToolExecutor(mode="approval_required")
    tool_runner = MappingToolExecutor.from_tools(get_default_tools())

    from abstractflow.visual.executor import create_visual_runner

    runner = create_visual_runner(
        vf,
        flows=flows,
        run_store=run_store,
        ledger_store=ledger_store,
        artifact_store=artifact_store,
        tool_executor=tool_executor,
    )
    runner._current_run_id = ref.run_id  # type: ignore[attr-defined]

    runtime = runner.runtime
    reg = getattr(runtime, "workflow_registry", None)
    if reg is None and hasattr(runtime, "set_workflow_registry"):
        try:
            from abstractruntime.scheduler.registry import WorkflowRegistry
        except Exception:
            WorkflowRegistry = None  # type: ignore[assignment]
        if WorkflowRegistry is not None:
            registry = WorkflowRegistry()
            registry.register(runner.workflow)
            runtime.set_workflow_registry(registry)

    if name:
        from abstractruntime.scheduler.scheduler import Scheduler

        scope2 = str(scope or "session").strip().lower() or "session"
        sess = session_id
        if sess is None and scope2 == "session":
            sess = ref.run_id

        scheduler = Scheduler(runtime=runtime, registry=runtime.workflow_registry)  # type: ignore[arg-type]
        resumed = scheduler.emit_event(
            name=str(name),
            payload=payload,
            scope=scope2,
            session_id=sess,
            max_steps=int(max_steps or 0),
        )
        print(f"Emitted event '{name}' scope={scope2} resumed={len(resumed)}")
    else:
        # Raw wait_key resumption: resume all WAITING EVENT runs that match this key.
        wk = str(wait_key or "").strip()
        if not wk:
            raise ValueError("--wait-key must be non-empty")

        from abstractruntime.core.models import RunStatus, WaitReason

        candidates = runtime.run_store.list_runs(status=RunStatus.WAITING, wait_reason=WaitReason.EVENT, limit=10_000)
        resumed_count = 0
        for r in candidates:
            if r.waiting is None or r.waiting.wait_key != wk:
                continue
            wf = _workflow_for(runtime, runner.workflow, r.workflow_id)
            runtime.resume(workflow=wf, run_id=r.run_id, wait_key=wk, payload=payload, max_steps=int(max_steps or 0))
            resumed_count += 1
        print(f"Resumed wait_key '{wk}' runs={resumed_count}")

    # Drive the session until it blocks again.
    _drive_until_blocked(runner=runner, tool_runner=tool_runner, auto_approve=bool(auto_approve), wait_until=False)
