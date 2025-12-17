"""Full-screen UI with scrollable history, fixed input, and status bar.

Uses prompt_toolkit's Application with HSplit layout to provide:
- Scrollable output/history area (mouse wheel + keyboard)
- Fixed input area at bottom
- Fixed status bar showing provider/model/context info
"""

from __future__ import annotations

from typing import Callable, Optional

from prompt_toolkit.application import Application
from prompt_toolkit.buffer import Buffer
from prompt_toolkit.formatted_text import HTML, FormattedText
from prompt_toolkit.key_binding import KeyBindings
from prompt_toolkit.layout.containers import HSplit, Window, ConditionalContainer
from prompt_toolkit.layout.controls import BufferControl, FormattedTextControl
from prompt_toolkit.layout.layout import Layout
from prompt_toolkit.layout.margins import ScrollbarMargin
from prompt_toolkit.styles import Style
from prompt_toolkit.widgets import TextArea


class FullScreenUI:
    """Full-screen chat interface with scrollable history."""

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

        # Output buffer (read-only, scrollable)
        self._output_buffer = Buffer(read_only=True, name="output")

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

    def _build_layout(self) -> None:
        """Build the HSplit layout with output, input, and status areas."""
        # Output area with scrollbar
        output_window = Window(
            content=BufferControl(buffer=self._output_buffer),
            right_margins=[ScrollbarMargin(display_arrows=True)],
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
        from prompt_toolkit.layout.containers import VSplit
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

        # Ctrl+Up = scroll up
        @self._kb.add("c-up")
        def scroll_up(event):
            self._scroll(-3)

        # Ctrl+Down = scroll down
        @self._kb.add("c-down")
        def scroll_down(event):
            self._scroll(3)

        # Page Up = scroll up more
        @self._kb.add("pageup")
        def page_up(event):
            self._scroll(-10)

        # Page Down = scroll down more
        @self._kb.add("pagedown")
        def page_down(event):
            self._scroll(10)

        # Alt+Enter = insert newline in input
        @self._kb.add("escape", "enter")
        def handle_alt_enter(event):
            self._input_buffer.insert_text("\n")

        # Ctrl+J = insert newline (Unix tradition)
        @self._kb.add("c-j")
        def handle_ctrl_j(event):
            self._input_buffer.insert_text("\n")

    def _scroll(self, lines: int) -> None:
        """Scroll the output window by N lines."""
        # Access the window's vertical scroll
        info = self._app.layout.current_window.render_info
        if info:
            # Scroll via the buffer's cursor position hack
            pass
        # Alternative: directly manipulate scroll offset
        # This is a bit hacky but works
        window = self._output_window
        if hasattr(window, 'vertical_scroll'):
            window.vertical_scroll = max(0, window.vertical_scroll + lines)

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
        current = self._output_buffer.text
        if current:
            new_text = current + "\n" + text
        else:
            new_text = text

        # Update buffer (need to temporarily make it writable)
        self._output_buffer.read_only = False
        self._output_buffer.set_document(
            self._output_buffer.document.__class__(
                text=new_text,
                cursor_position=len(new_text)
            )
        )
        self._output_buffer.read_only = True

    def clear_output(self) -> None:
        """Clear the output area."""
        self._output_buffer.read_only = False
        self._output_buffer.set_document(
            self._output_buffer.document.__class__(text="", cursor_position=0)
        )
        self._output_buffer.read_only = True

    def set_output(self, text: str) -> None:
        """Replace all output with new text."""
        self._output_buffer.read_only = False
        self._output_buffer.set_document(
            self._output_buffer.document.__class__(
                text=text,
                cursor_position=len(text)
            )
        )
        self._output_buffer.read_only = True

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
