from __future__ import annotations

from abstractcode.react_shell import _insert_archived_span


def test_insert_archived_span_inserts_after_matching_summary_and_dedups() -> None:
    active = [
        {"role": "system", "content": "sys", "metadata": {"message_id": "msg_sys"}},
        {
            "role": "system",
            "content": "[CONVERSATION HISTORY SUMMARY]: ...",
            "metadata": {
                "message_id": "msg_summary",
                "kind": "memory_summary",
                "source_artifact_id": "art_1",
            },
        },
        {"role": "user", "content": "recent", "metadata": {"message_id": "msg_recent"}},
    ]

    archived = [
        {
            "role": "user",
            "content": "old1",
            "timestamp": "2025-01-01T00:00:00Z",
            "metadata": {"message_id": "msg_old1"},
        },
        {
            "role": "assistant",
            "content": "old2",
            "timestamp": "2025-01-01T00:00:01Z",
            "metadata": {"message_id": "msg_old2"},
        },
        # Duplicate message_id should be skipped
        {
            "role": "assistant",
            "content": "dup",
            "timestamp": "2025-01-01T00:00:02Z",
            "metadata": {"message_id": "msg_recent"},
        },
    ]

    new_messages, inserted, skipped = _insert_archived_span(
        active_messages=active,
        archived_messages=archived,
        artifact_id="art_1",
    )

    assert inserted == 2
    assert skipped == 1
    assert new_messages[2]["metadata"]["message_id"] == "msg_old1"
    assert new_messages[3]["metadata"]["message_id"] == "msg_old2"

