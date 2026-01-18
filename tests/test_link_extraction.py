from __future__ import annotations

from abstractcode.react_shell import ReactShell


def test_extract_links_handles_markdown_and_bare_urls() -> None:
    shell = ReactShell.__new__(ReactShell)
    links = shell._extract_links("See [docs](https://a.example/x) and https://b.example/y).")
    assert links == ["https://a.example/x", "https://b.example/y"]

