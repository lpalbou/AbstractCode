from __future__ import annotations

import mimetypes

from pathlib import Path


def test_ingest_attachments_keeps_audio_as_audio_artifact(tmp_path: Path) -> None:
    """Audio attachments are stored as-is; STT happens at LLM call time in AbstractCore.

    This prevents AbstractCode from injecting "install X" notes or synthetic transcript artifacts
    into the LLM-visible context.
    """

    from abstractcode.react_shell import ReactShell
    from abstractruntime import InMemoryArtifactStore

    audio_path = tmp_path / "always.wav"
    audio_path.write_bytes(b"")

    shell = ReactShell.__new__(ReactShell)
    shell._artifact_store = InMemoryArtifactStore()
    shell._color = False
    shell._print = lambda *_a, **_k: None  # type: ignore[assignment]
    shell._max_attachment_bytes = lambda: 10_000_000  # type: ignore[assignment]
    shell._session_memory_run_id = lambda: "session_memory_test"  # type: ignore[assignment]
    shell._resolve_attachment_file = lambda _key: (audio_path, "always.wav")  # type: ignore[assignment]

    refs = shell._ingest_attachments(["always.wav"])

    assert len(refs) == 1
    ref = refs[0]
    assert ref.get("filename") == "always.wav"
    assert "$artifact" in ref
    assert ref.get("content_type") == (mimetypes.guess_type("always.wav")[0] or "application/octet-stream")
