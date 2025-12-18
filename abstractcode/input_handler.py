"""Input handling with prompt_toolkit for multi-line input and status footer."""

from __future__ import annotations

from typing import Callable, Optional

from prompt_toolkit import PromptSession
from prompt_toolkit.formatted_text import HTML
from prompt_toolkit.key_binding import KeyBindings
from prompt_toolkit.styles import Style


def create_prompt_session(
    get_toolbar_text: Callable[[], str],
    multiline: bool = True,
    color: bool = True,
) -> PromptSession:
    """Create a configured prompt session with multi-line input and status footer.

    Key bindings:
    - Enter: Submit input
    - Alt+Enter or Escape,Enter: Insert newline
    - Ctrl+J: Insert newline (Unix tradition)

    Args:
        get_toolbar_text: Callable that returns the toolbar text (called on each render)
        multiline: Enable multi-line input mode
        color: Enable colored output

    Returns:
        Configured PromptSession instance
    """
    kb = KeyBindings()

    # Enter = submit (override default multiline behavior where Enter adds newline)
    @kb.add("enter")
    def handle_enter(event):
        event.current_buffer.validate_and_handle()

    # Alt+Enter = insert newline
    @kb.add("escape", "enter")
    def handle_alt_enter(event):
        event.current_buffer.insert_text("\n")

    # Ctrl+J = insert newline (Unix tradition, works in all terminals)
    @kb.add("c-j")
    def handle_ctrl_j(event):
        event.current_buffer.insert_text("\n")

    # Style for the bottom toolbar
    style = Style.from_dict(
        {
            "bottom-toolbar": "bg:#1a1a2e #888888",
            "bottom-toolbar.text": "#888888",
        }
    ) if color else None

    return PromptSession(
        multiline=multiline,
        key_bindings=kb,
        bottom_toolbar=get_toolbar_text,
        style=style,
        mouse_support=True,
    )


def create_simple_session(color: bool = True) -> PromptSession:
    """Create a simple single-line prompt session for quick responses.

    Used for tool approvals, choice selection, etc.

    Args:
        color: Enable colored output

    Returns:
        Simple PromptSession instance
    """
    return PromptSession(
        multiline=False,
        mouse_support=False,
    )
