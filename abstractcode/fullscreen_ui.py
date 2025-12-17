"""Full-screen UI with scrollable history, fixed input, and status bar.

Uses prompt_toolkit's Application with HSplit layout to provide:
- Scrollable output/history area (mouse wheel + keyboard) with ANSI color support
- Fixed input area at bottom
- Fixed status bar showing provider/model/context info
"""

from __future__ import annotations

import re
from typing import Callable, List, Optional, Tuple

from prompt_toolkit.application import Application
from prompt_toolkit.buffer import Buffer
from prompt_toolkit.data_structures import Point
from prompt_toolkit.formatted_text import FormattedText, ANSI
from prompt_toolkit.key_binding import KeyBindings
from prompt_toolkit.layout.containers import HSplit, VSplit, Window
from prompt_toolkit.layout.controls import BufferControl, FormattedTextControl
from prompt_toolkit.layout.layout import Layout
from prompt_toolkit.styles import Style


class FullScreenUI:
    """Full-screen chat interface with scrollable history and ANSI color support."""

    def __init__(
        self,
        get_status_text: Callable[[], str],
        on_input: Callable[[str], None],
        color: bool = True,
    ):
        """Initialize the full-screen UI.

        Args:
            get_status_text: Callable that returns status bar text
            on_input: Callback when user submits input
            color: Enable colored output
        """
        self._get_status_text = get_status_text
        self._on_input = on_input
        self._color = color
        self._running = False

        # Output content storage (raw text with ANSI codes)
        self._output_text: str = ""
        # Scroll position (line offset from top)
        self._scroll_offset: int = 0

        # Input buffer
        self._input_buffer = Buffer(name="input", multiline=False)

        # Build the layout
        self._build_layout()
        self._build_keybindings()
        self._build_style()

        # Create application
        self._app = Application(
            layout=self._layout,
            key_bindings=self._kb,
            style=self._style,
            full_screen=True,
            mouse_support=True,
            erase_when_done=False,
        )

    def _get_output_formatted(self) -> FormattedText:
        """Get formatted output text with ANSI color support."""
        if not self._output_text:
            return FormattedText([])
        # Use ANSI class to parse escape codes into styled tuples
        return ANSI(self._output_text)

    def _get_cursor_position(self) -> Point:
        """Get cursor position for scrolling."""
        # Return position based on scroll offset
        return Point(0, self._scroll_offset)

    def _build_layout(self) -> None:
        """Build the HSplit layout with output, input, and status areas."""
        # Output area using FormattedTextControl for ANSI color support
        self._output_control = FormattedTextControl(
            text=self._get_output_formatted,
            focusable=True,
            get_cursor_position=self._get_cursor_position,
        )

        output_window = Window(
            content=self._output_control,
            wrap_lines=True,
        )

        # Separator line
        separator = Window(height=1, char="─", style="class:separator")

        # Input area
        input_window = Window(
            content=BufferControl(buffer=self._input_buffer),
            height=3,  # Allow a few lines for input
            wrap_lines=True,
        )

        # Input prompt label
        input_label = Window(
            content=FormattedTextControl(lambda: [("class:prompt", "> ")]),
            width=2,
            height=1,
        )

        # Combine input label and input window horizontally
        input_row = VSplit([input_label, input_window])

        # Status bar (fixed at bottom)
        status_bar = Window(
            content=FormattedTextControl(self._get_status_formatted),
            height=1,
            style="class:status-bar",
        )

        # Help hint bar
        help_bar = Window(
            content=FormattedTextControl(
                lambda: [("class:help", " Enter=submit | Ctrl+Up/Down=scroll | Ctrl+L=clear | Ctrl+C=exit")]
            ),
            height=1,
            style="class:help-bar",
        )

        # Stack everything vertically
        root = HSplit([
            output_window,    # Scrollable output (takes remaining space)
            separator,        # Visual separator
            input_row,        # Input area with prompt
            status_bar,       # Status info
            help_bar,         # Help hints
        ])

        self._layout = Layout(root)
        # Focus starts on input
        self._layout.focus(self._input_buffer)

        # Store references for later
        self._output_window = output_window

    def _get_status_formatted(self) -> FormattedText:
        """Get formatted status text."""
        text = self._get_status_text()
        return [("class:status-text", f" {text}")]

    def _build_keybindings(self) -> None:
        """Build key bindings."""
        self._kb = KeyBindings()

        # Enter = submit input
        @self._kb.add("enter")
        def handle_enter(event):
            text = self._input_buffer.text.strip()
            if text:
                # Clear input
                self._input_buffer.reset()
                # Process input (this will be handled async)
                self._pending_input = text
                event.app.exit(result=text)

        # Ctrl+C = exit
        @self._kb.add("c-c")
        def handle_ctrl_c(event):
            self._pending_input = None
            event.app.exit(result=None)

        # Ctrl+D = exit (EOF)
        @self._kb.add("c-d")
        def handle_ctrl_d(event):
            self._pending_input = None
            event.app.exit(result=None)

        # Ctrl+L = clear output
        @self._kb.add("c-l")
        def handle_ctrl_l(event):
            self.clear_output()
            event.app.invalidate()

        # Ctrl+Up = scroll up
        @self._kb.add("c-up")
        def scroll_up(event):
            self._scroll(-3)
            event.app.invalidate()

        # Ctrl+Down = scroll down
        @self._kb.add("c-down")
        def scroll_down(event):
            self._scroll(3)
            event.app.invalidate()

        # Page Up = scroll up more
        @self._kb.add("pageup")
        def page_up(event):
            self._scroll(-10)
            event.app.invalidate()

        # Page Down = scroll down more
        @self._kb.add("pagedown")
        def page_down(event):
            self._scroll(10)
            event.app.invalidate()

        # Alt+Enter = insert newline in input
        @self._kb.add("escape", "enter")
        def handle_alt_enter(event):
            self._input_buffer.insert_text("\n")

        # Ctrl+J = insert newline (Unix tradition)
        @self._kb.add("c-j")
        def handle_ctrl_j(event):
            self._input_buffer.insert_text("\n")

    def _scroll(self, lines: int) -> None:
        """Scroll the output by N lines."""
        # Count total lines in output
        total_lines = self._output_text.count('\n') + 1 if self._output_text else 0
        # Update scroll offset with bounds checking
        self._scroll_offset = max(0, min(total_lines - 1, self._scroll_offset + lines))

    def _build_style(self) -> None:
        """Build the style."""
        if self._color:
            self._style = Style.from_dict({
                "separator": "#444444",
                "status-bar": "bg:#1a1a2e #888888",
                "status-text": "#888888",
                "help-bar": "bg:#1a1a2e #666666",
                "help": "#666666 italic",
                "prompt": "#00aa00 bold",
            })
        else:
            self._style = Style.from_dict({})

    def append_output(self, text: str) -> None:
        """Append text to the output area."""
        if self._output_text:
            self._output_text += "\n" + text
        else:
            self._output_text = text
        # Auto-scroll to bottom when new content added
        total_lines = self._output_text.count('\n')
        self._scroll_offset = max(0, total_lines - 5)  # Keep some context visible

    def clear_output(self) -> None:
        """Clear the output area."""
        self._output_text = ""
        self._scroll_offset = 0

    def set_output(self, text: str) -> None:
        """Replace all output with new text."""
        self._output_text = text
        self._scroll_offset = 0

    def prompt(self) -> Optional[str]:
        """Show prompt and wait for input. Returns None on Ctrl+C/Ctrl+D."""
        try:
            result = self._app.run()
            return result
        except (EOFError, KeyboardInterrupt):
            return None

    def run_loop(self, banner: str = "") -> None:
        """Run the main input loop.

        Args:
            banner: Initial text to show in output
        """
        if banner:
            self.append_output(banner)

        self._running = True
        while self._running:
            user_input = self.prompt()
            if user_input is None:
                break
            self._on_input(user_input)

    def stop(self) -> None:
        """Stop the run loop."""
        self._running = False

    def exit(self) -> None:
        """Exit the application."""
        self._running = False
        if self._app.is_running:
            self._app.exit()
