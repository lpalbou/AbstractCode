from __future__ import annotations

import json
import os
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional, Tuple


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
                out[key] = _coerce_value(str(argv[i + 1]))
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
) -> Optional[Dict[str, Any]]:
    if auto_approve or approval_state.approve_all:
        return tool_runner.execute(tool_calls=tool_calls)

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
        if isinstance(payload, dict):
            exec_results = payload.get("results")
            if isinstance(exec_results, list):
                results.extend(exec_results)
        else:
            results.append({"call_id": "", "name": "tools", "success": False, "output": None, "error": "Invalid tool runner output"})

    return {"mode": "executed", "results": results}


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

    ledger_offsets: Dict[str, int] = {}
    approval = approval_state or _ApprovalState()

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
        _print_answer_user_records(runtime=runtime, run_ids=run_ids, offsets=ledger_offsets, emit=_emit_answer)

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
    prompt_fn: Any = None,
    ask_user_fn: Any = None,
    print_fn: Any = None,
    on_answer_user: Any = None,
) -> None:
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

    # Stores: file-backed only when state is enabled.
    state_path = Path(flow_state_file or default_flow_state_file()).expanduser().resolve()
    if no_state:
        run_store = InMemoryRunStore()
        ledger_store = InMemoryLedgerStore()
        artifact_store = InMemoryArtifactStore()
    else:
        state_path.parent.mkdir(parents=True, exist_ok=True)
        store_dir = state_path.with_name(state_path.stem + ".d")
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
    if not no_state:
        _save_flow_ref(state_path, FlowRunRef(flow_id=str(vf.id), flows_dir=str(flows_dir_path), run_id=run_id))

    try:
        _drive_until_blocked(
            runner=runner,
            tool_runner=tool_runner,
            auto_approve=auto_approve,
            wait_until=wait_until,
            prompt_fn=prompt_fn,
            ask_user_fn=ask_user_fn,
            print_fn=print_fn,
            on_answer_user=on_answer_user,
        )
    except KeyboardInterrupt:
        # Best-effort: pause the whole run tree so schedulers/event emitters won't advance it.
        try:
            for rid in _iter_descendants(runner.runtime, run_id):
                runner.runtime.pause_run(rid, reason="Paused via AbstractCode (KeyboardInterrupt)")
        except Exception:
            pass
        print("\nInterrupted. Run paused (best-effort).")


def resume_flow_command(
    *,
    flow_state_file: Optional[str],
    no_state: bool,
    auto_approve: bool,
    wait_until: bool,
    prompt_fn: Any = None,
    ask_user_fn: Any = None,
    print_fn: Any = None,
    on_answer_user: Any = None,
) -> None:
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

    store_dir = state_path.with_name(state_path.stem + ".d")
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
        prompt_fn=prompt_fn,
        ask_user_fn=ask_user_fn,
        print_fn=print_fn,
        on_answer_user=on_answer_user,
    )


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

    store_dir = state_path.with_name(state_path.stem + ".d")
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
