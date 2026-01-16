from __future__ import annotations

import tempfile
from pathlib import Path


def test_extract_at_file_mentions_strips_and_removes_tokens() -> None:
    from abstractcode.file_mentions import extract_at_file_mentions

    cleaned, mentions = extract_at_file_mentions("Summarize @secret, then check @docs/arch.md). Thanks!")
    assert mentions == ["secret", "docs/arch.md"]
    assert cleaned == "Summarize then check Thanks!"


def test_search_workspace_files_ranks_basename_prefix_first() -> None:
    from abstractcode.file_mentions import search_workspace_files

    files = ["docs/secret.md", "secret", "misc/other.txt"]
    assert search_workspace_files(files, "sec", limit=10)[:2] == ["secret", "docs/secret.md"]


def test_list_workspace_files_respects_abstractignore() -> None:
    from abstractcode.file_mentions import list_workspace_files
    from abstractcore.tools.abstractignore import AbstractIgnore

    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        (root / "a.txt").write_text("a", encoding="utf-8")
        (root / "b.txt").write_text("b", encoding="utf-8")
        (root / ".abstractignore").write_text("b.txt\n", encoding="utf-8")

        (root / "node_modules").mkdir()
        (root / "node_modules" / "x.txt").write_text("x", encoding="utf-8")

        ignore = AbstractIgnore.for_path(root)
        out = list_workspace_files(root=root, ignore=ignore, max_files=1000)

        assert "a.txt" in out
        assert "b.txt" not in out
        assert "node_modules/x.txt" not in out

