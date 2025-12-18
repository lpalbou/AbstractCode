"""Real-LLM integration test for AbstractCode compaction (no mocks).

Validates that `/compact`-equivalent logic:
- updates the run-backed active context (`RunState.vars["context"]["messages"]`)
- archives the summarized span into ArtifactStore
- stores provenance metadata in the inserted summary message
"""

from __future__ import annotations

import os
import uuid
from datetime import datetime, timezone
from typing import Any

import pytest


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def _skip_if_llm_unavailable(exc: Exception) -> None:
    msg = str(exc).lower()
    if any(
        keyword in msg
        for keyword in (
            "connection",
            "refused",
            "timeout",
            "timed out",
            "not running",
            "operation not permitted",
            "no such host",
            "not found",
            "model not found",
            "pull",
            "failed to connect",
        )
    ):
        pytest.skip(f"Local LLM not available: {exc}")


def _skip_if_llm_error_text(text: str) -> None:
    """Skip when the provider returns an error as plain text (no exception raised)."""
    msg = (text or "").lower()
    if any(
        keyword in msg
        for keyword in (
            "connection",
            "refused",
            "timeout",
            "timed out",
            "not running",
            "operation not permitted",
            "no such host",
            "not found",
            "model not found",
            "pull",
            "failed to connect",
        )
    ):
        pytest.skip(f"Local LLM not available (error text): {text}")


@pytest.mark.integration
def test_compact_updates_active_context_and_archives_span() -> None:
    from abstractcode.react_shell import ReactShell
    from abstractruntime.core.models import RunState, RunStatus

    provider = os.getenv("ABSTRACTCODE_TEST_PROVIDER", "ollama")
    model = os.getenv("ABSTRACTCODE_TEST_MODEL", "qwen3:4b-instruct-2507-q4_K_M")

    # ReactShell initializes the runtime and queries model capabilities; skip if local LLM is not reachable.
    try:
        shell = ReactShell(
            agent="react",
            provider=provider,
            model=model,
            state_file=None,  # in-memory stores for test
            auto_approve=True,
            max_iterations=5,
            max_tokens=32768,
            color=False,
        )
    except Exception as e:
        _skip_if_llm_unavailable(e)
        raise

    run_id = f"run_{uuid.uuid4().hex}"
    token = uuid.uuid4().hex

    # Seed a completed run with a non-trivial active context.
    messages: list[dict[str, Any]] = []
    for i in range(12):
        role = "user" if i % 2 == 0 else "assistant"
        messages.append(
            {
                "role": role,
                "content": f"{role} message {i} ({token})",
                "timestamp": _now_iso(),
                "metadata": {},
            }
        )

    state = RunState(
        run_id=run_id,
        workflow_id=shell._agent.workflow.workflow_id,
        status=RunStatus.COMPLETED,
        current_node="done",
        vars={
            "context": {"task": "test", "messages": messages},
            "scratchpad": {"iteration": 0, "max_iterations": 25},
            "_runtime": {},
            "_temp": {},
            "_limits": {},
        },
        waiting=None,
        output={"messages": messages},
        error=None,
        created_at=_now_iso(),
        updated_at=_now_iso(),
        actor_id=None,
        session_id=None,
        parent_run_id=None,
    )

    shell._runtime.run_store.save(state)
    shell._agent.attach(run_id)

    # Capture the post-attach view: attach may inject system messages into the active context.
    pre = shell._runtime.run_store.load(run_id)
    assert pre is not None
    pre_active = pre.vars.get("context", {}).get("messages", [])
    assert isinstance(pre_active, list)

    preserve_recent = 2
    system_count = sum(1 for m in pre_active if isinstance(m, dict) and m.get("role") == "system")
    conversation_count = sum(1 for m in pre_active if isinstance(m, dict) and m.get("role") != "system")
    expected_active_len = system_count + 1 + preserve_recent
    expected_archived = max(0, conversation_count - preserve_recent)

    result = shell._handle_compact(f"standard --preserve {preserve_recent}")
    if isinstance(result, dict) and not result.get("ok", True):
        _skip_if_llm_unavailable(RuntimeError(str(result.get("error") or "unknown error")))
    if isinstance(result, dict):
        comp_run_id = result.get("comp_run_id")
        meta_out = result.get("meta") if isinstance(result.get("meta"), dict) else {}
        llm_run_id = meta_out.get("llm_run_id") if isinstance(meta_out, dict) else None

        if isinstance(comp_run_id, str) and comp_run_id:
            ledger = shell._runtime.get_ledger(comp_run_id)
            assert any((r.get("effect") or {}).get("type") == "memory_compact" for r in ledger)

        if isinstance(llm_run_id, str) and llm_run_id:
            llm_ledger = shell._runtime.get_ledger(llm_run_id)
            assert any((r.get("effect") or {}).get("type") == "llm_call" for r in llm_ledger)

    updated = shell._runtime.run_store.load(run_id)
    assert updated is not None

    active = updated.vars.get("context", {}).get("messages", [])
    assert isinstance(active, list)

    # Expect: preserved system messages + 1 summary + preserved recent messages.
    assert len(active) == expected_active_len

    summary_candidates = [
        m
        for m in active
        if isinstance(m, dict) and isinstance((m.get("metadata") or {}), dict) and (m.get("metadata") or {}).get("kind") == "memory_summary"
    ]
    assert len(summary_candidates) == 1
    summary_msg = summary_candidates[0]
    assert isinstance(summary_msg, dict)
    assert summary_msg.get("role") == "system"
    meta = summary_msg.get("metadata") or {}
    assert isinstance(meta, dict)
    assert meta.get("kind") == "memory_summary"
    _skip_if_llm_error_text(str(summary_msg.get("content") or ""))

    # Provenance: summary references a stored artifact span.
    artifact_id = meta.get("source_artifact_id")
    assert isinstance(artifact_id, str) and artifact_id
    assert isinstance(meta.get("source_from_message_id"), str)
    assert isinstance(meta.get("source_to_message_id"), str)
    assert f"span_id={artifact_id}" in str(summary_msg.get("content") or "")

    runtime_ns = updated.vars.get("_runtime") or {}
    assert isinstance(runtime_ns, dict)
    spans = runtime_ns.get("memory_spans") or []
    assert isinstance(spans, list) and spans
    match = next((s for s in spans if isinstance(s, dict) and s.get("artifact_id") == artifact_id), None)
    assert match is not None
    assert match.get("summary_message_id") == meta.get("message_id")

    archived = shell._artifact_store.load_json(artifact_id)
    assert isinstance(archived, dict)
    assert isinstance(archived.get("messages"), list)
    assert archived.get("span", {}).get("message_count") == expected_archived

    # Rehydrate the archived span back into active context and persist.
    shell._handle_expand("1 --into-context")
    expanded = shell._runtime.run_store.load(run_id)
    assert expanded is not None
    expanded_active = expanded.vars.get("context", {}).get("messages", [])
    assert isinstance(expanded_active, list)
    # Expanded returns to: system messages + summary + original conversation messages.
    assert len(expanded_active) == len(pre_active) + 1
