from __future__ import annotations

from abstractcode.fullscreen_ui import FullScreenUI


def _flatten_text(formatted) -> str:
    parts: list[str] = []
    for frag in formatted:
        if not isinstance(frag, tuple) or len(frag) < 2:
            continue
        txt = frag[1]
        if isinstance(txt, str):
            parts.append(txt)
    return "".join(parts)


def test_format_output_text_linkifies_urls_and_keeps_punctuation() -> None:
    ui = FullScreenUI.__new__(FullScreenUI)

    formatted = ui._format_output_text("See https://example.com/path).")

    # The original text should be preserved.
    assert _flatten_text(formatted) == "See https://example.com/path)."

    # The URL should be a clickable fragment (handler attached) without trailing punctuation.
    clickable = [frag for frag in formatted if isinstance(frag, tuple) and len(frag) >= 3]
    assert any(frag[1] == "https://example.com/path" for frag in clickable)

