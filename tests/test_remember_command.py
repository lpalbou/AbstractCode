from __future__ import annotations


def test_parse_remember_args_and_store_note_into_target_run() -> None:
    from abstractcode.remember import parse_remember_args, store_memory_note

    from abstractruntime import Runtime
    from abstractruntime.core.models import RunState, RunStatus
    from abstractruntime.storage.artifacts import InMemoryArtifactStore
    from abstractruntime.storage.in_memory import InMemoryLedgerStore, InMemoryRunStore

    req = parse_remember_args('Important decision --tag topic=api --tag person=alice --last 3 --scope run')
    assert req.note == "Important decision"
    assert req.tags == {"topic": "api", "person": "alice"}
    assert req.last_messages == 3
    assert req.scope == "run"

    run_store = InMemoryRunStore()
    ledger_store = InMemoryLedgerStore()
    runtime = Runtime(run_store=run_store, ledger_store=ledger_store)
    artifact_store = InMemoryArtifactStore()
    runtime.set_artifact_store(artifact_store)

    target_run_id = "run_target"
    run_store.save(
        RunState(
            run_id=target_run_id,
            workflow_id="wf",
            status=RunStatus.COMPLETED,
            current_node="done",
            vars={
                "context": {"task": "t", "messages": []},
                "scratchpad": {},
                "_runtime": {"memory_spans": []},
                "_temp": {},
                "_limits": {},
            },
            waiting=None,
            output={"messages": []},
            error=None,
            created_at="2025-01-01T00:00:00+00:00",
            updated_at="2025-01-01T00:00:00+00:00",
            actor_id=None,
            session_id=None,
            parent_run_id=None,
        )
    )

    sources = {"run_id": target_run_id, "span_ids": [], "message_ids": ["m1", "m2", "m3"]}
    result = store_memory_note(
        runtime=runtime,
        target_run_id=target_run_id,
        note=req.note,
        tags=req.tags,
        sources=sources,
        actor_id=None,
        session_id=None,
        call_id="remember",
        scope=req.scope,
    )

    assert result.get("mode") == "executed"
    results = result.get("results")
    assert isinstance(results, list) and results
    first = results[0]
    assert first.get("success") is True

    span_id = (first.get("meta") or {}).get("span_id")
    assert isinstance(span_id, str) and span_id

    updated_target = run_store.load(target_run_id)
    assert updated_target is not None
    spans = updated_target.vars.get("_runtime", {}).get("memory_spans")
    assert isinstance(spans, list) and spans
    assert spans[0].get("artifact_id") == span_id

    payload = artifact_store.load_json(span_id)
    assert isinstance(payload, dict)
    assert payload.get("note") == "Important decision"

