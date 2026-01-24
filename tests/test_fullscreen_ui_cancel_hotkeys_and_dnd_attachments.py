from __future__ import annotations

import queue
from pathlib import Path

from prompt_toolkit.buffer import Buffer

from abstractcode.fullscreen_ui import BLOCKING_PROMPT_CANCEL_TOKEN, FullScreenUI


def _ui_stub() -> FullScreenUI:
    ui = FullScreenUI.__new__(FullScreenUI)
    ui._app = None
    ui._input_buffer = Buffer(name="input")
    ui._attachments = []
    ui._last_ctrl_c_at = None
    ui._suppress_attachment_draft_detection = False
    ui._pending_blocking_prompt = None
    ui._on_cancel = None
    # Workspace policy for attachment resolution.
    ui._workspace_root = Path.cwd()
    ui._workspace_mounts = {}
    ui._workspace_mount_ignores = {}
    ui._workspace_blocked_paths = []
    ui._workspace_ignore = None
    return ui


def test_ctrl_c_clears_draft_then_exits_on_second_press_within_window() -> None:
    ui = _ui_stub()
    ui._attachments = ["keep.txt"]

    ui._input_buffer.insert_text("hello")
    assert ui._ctrl_c_should_exit(now=0.0) is False
    assert ui._input_buffer.text == ""
    assert ui._attachments == ["keep.txt"]

    assert ui._ctrl_c_should_exit(now=0.5) is True


def test_ctrl_c_does_not_exit_when_second_press_is_outside_window() -> None:
    ui = _ui_stub()
    ui._input_buffer.insert_text("hello")
    assert ui._ctrl_c_should_exit(now=0.0) is False
    assert ui._input_buffer.text == ""

    assert ui._ctrl_c_should_exit(now=2.0) is False


def test_escape_request_cancel_unblocks_blocking_prompt_and_calls_callback() -> None:
    ui = _ui_stub()
    q: queue.Queue[str] = queue.Queue()
    ui._pending_blocking_prompt = q

    called: list[bool] = []

    def _cb() -> None:
        called.append(True)

    ui._on_cancel = _cb

    ui.request_cancel()

    assert called == [True]
    assert q.get_nowait() == BLOCKING_PROMPT_CANCEL_TOKEN


def test_bracketed_paste_absolute_paths_become_attachment_chips(tmp_path: Path) -> None:
    ui = _ui_stub()
    ui._workspace_root = tmp_path

    p1 = tmp_path / "a.txt"
    p1.write_text("a")
    p2 = tmp_path / "with space.txt"
    p2.write_text("b")

    assert ui.maybe_add_attachments_from_paste(str(p1)) is True
    assert ui._attachments == ["a.txt"]

    escaped = str(p2).replace(" ", "\\ ")
    assert ui.maybe_add_attachments_from_paste(escaped) is True
    assert ui._attachments == ["a.txt", "with space.txt"]


def test_bracketed_paste_out_of_workspace_paths_become_absolute_attachment_chips(tmp_path: Path) -> None:
    ui = _ui_stub()
    ui._workspace_root = tmp_path

    other_dir = tmp_path / "other"
    other_dir.mkdir()
    other_file = other_dir / "nope.txt"
    other_file.write_text("nope")

    # Make the file outside the workspace root by pointing workspace_root to a sibling.
    ui._workspace_root = other_dir
    outside = tmp_path / "outside.txt"
    outside.write_text("x")

    assert ui.maybe_add_attachments_from_paste(str(outside)) is True
    assert ui._attachments == [str(outside.resolve())]


def test_bracketed_paste_non_paths_fall_back_to_normal_paste() -> None:
    ui = _ui_stub()
    assert ui.maybe_add_attachments_from_paste("hello world") is False
    assert ui._attachments == []


def test_non_bracketed_paste_path_in_draft_converts_to_attachment_and_clears_draft(tmp_path: Path) -> None:
    ui = _ui_stub()
    ui._workspace_root = tmp_path
    ui._input_buffer.on_text_changed += ui._on_input_buffer_text_changed  # type: ignore[operator]

    p = tmp_path / "dropped.txt"
    p.write_text("hi")

    ui._input_buffer.insert_text(str(p))

    assert ui._attachments == ["dropped.txt"]
    assert ui._input_buffer.text == ""


def test_non_bracketed_drop_path_in_mixed_draft_extracts_attachment_and_preserves_text(tmp_path: Path) -> None:
    ui = _ui_stub()
    ui._workspace_root = tmp_path
    ui._input_buffer.on_text_changed += ui._on_input_buffer_text_changed  # type: ignore[operator]

    p = tmp_path / "dropped.txt"
    p.write_text("hi")

    ui._input_buffer.insert_text("summarize ")
    ui._input_buffer.insert_text(str(p))

    assert ui._attachments == ["dropped.txt"]
    assert str(p) not in ui._input_buffer.text
    assert ui._input_buffer.text.strip() == "summarize"


def test_non_bracketed_drop_path_with_escaped_spaces_in_mixed_draft_extracts_attachment(tmp_path: Path) -> None:
    ui = _ui_stub()
    ui._workspace_root = tmp_path
    ui._input_buffer.on_text_changed += ui._on_input_buffer_text_changed  # type: ignore[operator]

    p = tmp_path / "with space.txt"
    p.write_text("hi")

    dropped = str(p).replace(" ", "\\ ")
    ui._input_buffer.insert_text("summarize ")
    ui._input_buffer.insert_text(dropped)

    assert ui._attachments == ["with space.txt"]
    assert dropped not in ui._input_buffer.text
    assert ui._input_buffer.text.strip() == "summarize"


def test_non_bracketed_drop_outside_workspace_in_mixed_draft_extracts_absolute_attachment(tmp_path: Path) -> None:
    ui = _ui_stub()
    workspace = tmp_path / "workspace"
    workspace.mkdir()
    ui._workspace_root = workspace
    ui._input_buffer.on_text_changed += ui._on_input_buffer_text_changed  # type: ignore[operator]

    outside = tmp_path / "outside.txt"
    outside.write_text("x")

    ui._input_buffer.insert_text("check ")
    ui._input_buffer.insert_text(str(outside))

    assert ui._attachments == [str(outside.resolve())]
    assert str(outside) not in ui._input_buffer.text
    assert ui._input_buffer.text.strip() == "check"
