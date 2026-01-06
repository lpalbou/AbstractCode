"""AbstractCode remember helpers (no LLM required).

AbstractCode is a host UX; "remember" should be implemented via runtime-owned
memory primitives so behavior stays consistent across hosts.

This module provides:
- lightweight argument parsing for `/memorize`
- an execution helper that stores a runtime `MEMORY_NOTE` targeting a run
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Dict, Optional


@dataclass(frozen=True)
class RememberRequest:
    note: str
    tags: Dict[str, str]
    span_id: Optional[str] = None
    last_span: bool = False
    last_messages: int = 6
    scope: str = "run"  # run|session|global

    def __post_init__(self) -> None:
        object.__setattr__(self, "note", str(self.note or "").strip())
        object.__setattr__(self, "tags", dict(self.tags or {}))
        raw_scope = str(getattr(self, "scope", "") or "").strip().lower() or "run"
        if raw_scope not in ("run", "session", "global"):
            raw_scope = "run"
        object.__setattr__(self, "scope", raw_scope)


def parse_remember_args(raw: str) -> RememberRequest:
    """Parse `/memorize` arguments.

    Syntax:
      /memorize <note text> [--tag k=v ...] [--span <span_id>] [--last-span] [--last N] [--scope run|session|global]

    Notes:
    - Note text may be quoted, but quoting is optional (we treat all non-flag tokens as note text).
    - Tags are JSON-safe `str -> str` and `kind=...` is ignored (reserved).
    """
    import shlex

    try:
        parts = shlex.split(raw) if raw else []
    except ValueError:
        parts = raw.split() if raw else []

    tags: Dict[str, str] = {}
    span_id: Optional[str] = None
    last_span = False
    last_messages = 6
    scope = "run"
    note_parts: list[str] = []

    i = 0
    while i < len(parts):
        p = str(parts[i])
        if p in ("--tag", "--tags"):
            if i + 1 >= len(parts):
                raise ValueError("--tag requires k=v")
            kv = str(parts[i + 1])
            if "=" not in kv:
                raise ValueError("--tag requires k=v")
            k, v = kv.split("=", 1)
            key = k.strip()
            val = v.strip()
            if not key or not val:
                raise ValueError("--tag requires non-empty k=v")
            if key != "kind":
                tags[key] = val
            i += 2
            continue
        if p == "--span":
            if i + 1 >= len(parts):
                raise ValueError("--span requires a span_id")
            span_id = str(parts[i + 1]).strip() or None
            i += 2
            continue
        if p == "--last-span":
            last_span = True
            i += 1
            continue
        if p == "--last":
            if i + 1 >= len(parts):
                raise ValueError("--last requires a number")
            try:
                last_messages = int(parts[i + 1])
            except Exception as e:
                raise ValueError("--last requires a number") from e
            if last_messages < 0:
                last_messages = 0
            i += 2
            continue
        if p == "--scope":
            if i + 1 >= len(parts):
                raise ValueError("--scope requires a value")
            scope = str(parts[i + 1]).strip().lower() or "run"
            if scope not in ("run", "session", "global"):
                raise ValueError("--scope must be run|session|global")
            i += 2
            continue
        if p.startswith("--"):
            raise ValueError(f"Unknown flag: {p}")

        note_parts.append(p)
        i += 1

    note = " ".join([p for p in note_parts if p]).strip()
    if not note:
        raise ValueError("note text is required")

    return RememberRequest(note=note, tags=tags, span_id=span_id, last_span=last_span, last_messages=last_messages, scope=scope)


def store_memory_note(
    *,
    runtime: Any,
    target_run_id: str,
    note: str,
    tags: Dict[str, str],
    sources: Dict[str, Any],
    actor_id: Optional[str],
    session_id: Optional[str],
    call_id: str = "memorize",
    scope: str = "run",
) -> Dict[str, Any]:
    """Store a runtime memory note targeting `target_run_id`.

    This is implemented as a tiny child workflow that emits `EffectType.MEMORY_NOTE`
    with `payload.target_run_id`, so the runtime stores the note on the target run.
    """
    from abstractruntime import Effect, EffectType, StepPlan, WorkflowSpec
    from abstractruntime.core.models import RunStatus

    payload: Dict[str, Any] = {
        "target_run_id": str(target_run_id),
        "note": str(note or ""),
        "tags": dict(tags or {}),
        "sources": dict(sources or {}),
        "scope": str(scope or "run"),
        "tool_name": "remember_note",
        "call_id": str(call_id or "remember"),
    }

    def remember_node(run, ctx) -> StepPlan:
        return StepPlan(
            node_id="remember",
            effect=Effect(type=EffectType.MEMORY_NOTE, payload=payload, result_key="_temp.remember"),
            next_node="done",
        )

    def done_node(run, ctx) -> StepPlan:
        temp = run.vars.get("_temp")
        if not isinstance(temp, dict):
            temp = {}
        return StepPlan(node_id="done", complete_output={"result": temp.get("remember")})

    wf = WorkflowSpec(
        workflow_id="abstractcode_remember_command",
        entry_node="remember",
        nodes={"remember": remember_node, "done": done_node},
    )

    remember_run_id = runtime.start(
        workflow=wf,
        vars={"context": {}, "scratchpad": {}, "_runtime": {}, "_temp": {}, "_limits": {}},
        actor_id=actor_id,
        session_id=session_id,
        parent_run_id=str(target_run_id),
    )
    st = runtime.tick(workflow=wf, run_id=remember_run_id, max_steps=50)
    if st.status != RunStatus.COMPLETED:
        raise RuntimeError(st.error or "remember_note failed")

    out = st.output or {}
    result = out.get("result") if isinstance(out, dict) else None
    if not isinstance(result, dict):
        return {"remember_run_id": remember_run_id, "result": result}
    return {"remember_run_id": remember_run_id, **result}

