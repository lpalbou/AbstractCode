from __future__ import annotations

from typing import Any

import pytest

from abstractcode.recall import execute_recall, parse_recall_args
from abstractruntime.core.models import RunState, RunStatus
from abstractruntime.storage.artifacts import InMemoryArtifactStore
from abstractruntime.storage.in_memory import InMemoryRunStore


def test_parse_recall_args_supports_time_tags_query_and_into_context() -> None:
    req = parse_recall_args(
        "--since 2025-01-01T00:00:00+00:00 --until 2025-01-02T00:00:00+00:00 "
        "--tag topic=r-type --tag person=alice --q player dies --limit 3 --into-context --placement end --show --scope all"
    )

    assert req.since == "2025-01-01T00:00:00+00:00"
    assert req.until == "2025-01-02T00:00:00+00:00"
    assert req.tags == {"topic": "r-type", "person": "alice"}
    assert req.tags_mode == "all"
    assert req.users == []
    assert req.locations == []
    assert req.query == "player dies"
    assert req.limit == 3
    assert req.into_context is True
    assert req.placement == "end"
    assert req.show is True
    assert req.scope == "all"


def test_parse_recall_args_uses_leftovers_as_query_when_missing_q_flag() -> None:
    req = parse_recall_args("--tag topic=r-type player dies instantly")
    assert req.tags == {"topic": "r-type"}
    assert req.query == "player dies instantly"


def test_parse_recall_args_rejects_invalid_iso() -> None:
    with pytest.raises(ValueError):
        parse_recall_args("--since not-a-time")


def test_parse_recall_args_supports_tags_mode_users_locations_and_multi_value_tags() -> None:
    req = parse_recall_args(
        "--tags-mode any "
        "--tag person=alice --tag person=bob "
        "--user alice --user bob "
        "--location paris --location nyc "
        "--q opinions"
    )
    assert req.tags_mode == "any"
    assert req.tags == {"person": ["alice", "bob"]}
    assert req.users == ["alice", "bob"]
    assert req.locations == ["paris", "nyc"]
    assert req.query == "opinions"


def test_execute_recall_filters_spans_and_rehydrates_messages() -> None:
    run_id = "run_recall"
    run_store = InMemoryRunStore()
    artifact_store = InMemoryArtifactStore()

    created_at = "2025-01-01T00:00:00+00:00"
    span_art = artifact_store.store_json(
        {
            "messages": [
                {"role": "user", "content": "old1", "timestamp": created_at, "metadata": {"message_id": "m_old1"}},
                {"role": "assistant", "content": "old2", "timestamp": created_at, "metadata": {"message_id": "m_old2"}},
                # Duplicate message_id should be skipped during rehydration.
                {"role": "user", "content": "dup", "timestamp": created_at, "metadata": {"message_id": "m_recent"}},
            ]
        },
        run_id=run_id,
        tags={"kind": "conversation_span", "topic": "r-type"},
    )

    note_art = artifact_store.store_json(
        {
            "note": "Remember: Alice owns the API contract.",
            "sources": {"run_id": run_id, "span_ids": [span_art.artifact_id], "message_ids": ["m_old1"]},
            "created_at": created_at,
        },
        run_id=run_id,
        tags={"kind": "memory_note", "topic": "r-type", "person": "alice"},
    )

    vars: dict[str, Any] = {
        "context": {
            "task": "t",
            "messages": [
                {"role": "system", "content": "sys", "metadata": {"message_id": "m_sys"}},
                {
                    "role": "system",
                    "content": f"[CONVERSATION HISTORY SUMMARY span_id={span_art.artifact_id}] Key points: player dies instantly.",
                    "metadata": {"message_id": "m_summary", "kind": "memory_summary", "source_artifact_id": span_art.artifact_id},
                },
                {"role": "user", "content": "recent", "timestamp": created_at, "metadata": {"message_id": "m_recent"}},
            ],
        },
        "scratchpad": {},
        "_runtime": {
            "memory_spans": [
                {
                    "kind": "conversation_span",
                    "artifact_id": span_art.artifact_id,
                    "created_at": created_at,
                    "from_timestamp": created_at,
                    "to_timestamp": created_at,
                    "message_count": 3,
                    "tags": {"topic": "r-type"},
                    "summary_message_id": "m_summary",
                },
                {
                    "kind": "memory_note",
                    "artifact_id": note_art.artifact_id,
                    "created_at": created_at,
                    "from_timestamp": created_at,
                    "to_timestamp": created_at,
                    "message_count": 0,
                    "tags": {"topic": "r-type", "person": "alice"},
                    "note_preview": "Remember: Alice owns the API contract.",
                },
            ]
        },
        "_temp": {},
        "_limits": {},
    }

    run_store.save(
        RunState(
            run_id=run_id,
            workflow_id="wf",
            status=RunStatus.COMPLETED,
            current_node="done",
            vars=vars,
            waiting=None,
            output={"messages": vars["context"]["messages"]},
            error=None,
            created_at=created_at,
            updated_at=created_at,
            actor_id=None,
            session_id=None,
            parent_run_id=None,
        )
    )

    # Query should match the note (text) and the span (summary text) + tag filter.
    req = parse_recall_args("--tag topic=r-type r-type")
    res = execute_recall(run_id=run_id, run_store=run_store, artifact_store=artifact_store, request=req)
    matches = res.get("matches")
    assert isinstance(matches, list)
    assert {m.get("artifact_id") for m in matches if isinstance(m, dict)} == {span_art.artifact_id, note_art.artifact_id}

    # Rehydrate should insert the conversation span's archived messages, but not the memory_note.
    req2 = parse_recall_args("--tag topic=r-type player dies --into-context")
    res2 = execute_recall(run_id=run_id, run_store=run_store, artifact_store=artifact_store, request=req2)
    rehydration = res2.get("rehydration")
    assert isinstance(rehydration, dict)
    # `memory_note` spans include their linked conversation summary text in the query haystack,
    # so searching for "player dies" surfaces both the archived span and the derived note.
    assert rehydration.get("inserted") == 3
    assert rehydration.get("skipped") == 1

    updated = run_store.load(run_id)
    assert updated is not None
    active = updated.vars.get("context", {}).get("messages")
    assert isinstance(active, list)
    # The rehydrated messages should now exist and carry provenance markers.
    old1 = next((m for m in active if isinstance(m, dict) and (m.get("metadata") or {}).get("message_id") == "m_old1"), None)
    assert isinstance(old1, dict)
    meta = old1.get("metadata") or {}
    assert isinstance(meta, dict)
    assert meta.get("rehydrated") is True
    assert meta.get("source_artifact_id") == span_art.artifact_id
    # Notes are rehydrated as a synthetic system message (LLM-visible).
    note_msg = next(
        (
            m
            for m in active
            if isinstance(m, dict) and (m.get("metadata") or {}).get("message_id") == f"memory_note:{note_art.artifact_id}"
        ),
        None,
    )
    assert isinstance(note_msg, dict)

