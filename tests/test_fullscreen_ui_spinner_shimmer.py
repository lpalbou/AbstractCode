from __future__ import annotations

from abstractcode.fullscreen_ui import FullScreenUI


def _highlighted_spinner_indices(formatted) -> set[int]:
    """Return indices (within spinner text) that are currently highlighted.

    The status bar formats spinner text as a sequence of fragments:
    - ("class:spinner", " <glyph> ")
    - ("class:spinner-text", "...")
    - ("class:spinner-text-highlight", "...")
    - ("class:status-text", "  │  ...")
    """
    idx = 0
    highlighted: set[int] = set()

    for frag in formatted:
        if not isinstance(frag, tuple) or len(frag) < 2:
            continue
        style, text = frag[0], frag[1]
        if style not in ("class:spinner-text", "class:spinner-text-highlight"):
            continue
        if not isinstance(text, str):
            continue
        if style == "class:spinner-text-highlight":
            highlighted.update(range(idx, idx + len(text)))
        idx += len(text)

    return highlighted


def test_spinner_shimmer_reaches_end_of_long_text() -> None:
    """Regression test: shimmer must traverse the *entire* spinner text.

    The shimmer position is driven by an internal frame counter. If that counter is
    incorrectly wrapped by the number of spinner glyph frames (often 10), the shimmer
    can never reach the end of longer texts.
    """
    ui = FullScreenUI.__new__(FullScreenUI)

    # Minimal state for `_get_status_formatted`.
    ui._get_status_text = lambda: "provider | model"
    ui._spinner_frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
    ui._spinner_active = True
    ui._spinner_frame = 0

    # Make the spinner text longer than the number of spinner glyph frames, so
    # a modulo bug would prevent shimmering all the way to the right.
    ui._spinner_text = "A" * (len(ui._spinner_frames) + 5)

    highlighted: set[int] = set()
    # Advance enough frames to cover the full text at least once.
    for _ in range(len(ui._spinner_text) * 2):
        ui._advance_spinner_frame()
        highlighted |= _highlighted_spinner_indices(ui._get_status_formatted())

    assert (len(ui._spinner_text) - 1) in highlighted



