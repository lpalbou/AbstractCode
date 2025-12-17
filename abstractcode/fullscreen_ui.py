"""Full-screen UI with scrollable history, fixed input, and status bar.

Uses prompt_toolkit's Application with HSplit layout to provide:
- Scrollable output/history area (mouse wheel + keyboard) with ANSI color support
- Fixed input area at bottom
- Fixed status bar showing provider/model/context info
- Command autocomplete when typing /
"""

from __future__ import annotations

import queue
import re
import threading
import time
from typing import Callable, List, Optional, Tuple

from prompt_toolkit.application import Application
from prompt_toolkit.buffer import Buffer
from prompt_toolkit.completion import Completer, Completion
from prompt_toolkit.filters import has_completions
from prompt_toolkit.history import InMemoryHistory
from prompt_toolkit.data_structures import Point
from prompt_toolkit.formatted_text import FormattedText, ANSI
from prompt_toolkit.key_binding import KeyBindings
from prompt_toolkit.layout.containers import Float, FloatContainer, HSplit, VSplit, Window
from prompt_toolkit.layout.controls import BufferControl, FormattedTextControl
from prompt_toolkit.layout.layout import Layout
from prompt_toolkit.layout.menus import CompletionsMenu
from prompt_toolkit.styles import Style


# Command definitions: (command, description)
COMMANDS = [
    ("help", "Show available commands"),
    ("tools", "List available tools"),
    ("status", "Show current run status"),
    ("history", "Show recent conversation history"),
    ("resume", "Resume the saved/attached run"),
    ("clear", "Clear memory and start fresh"),
    ("compact", "Compress conversation [light|standard|heavy] [--preserve N] [focus...]"),
    ("new", "Start fresh (alias for /clear)"),
    ("reset", "Reset session (alias for /clear)"),
    ("task", "Start a new task"),
    ("auto-accept", "Toggle auto-accept for tools [saved]"),
    ("max-tokens", "Show or set max tokens (-1 = auto) [saved]"),
    ("max-messages", "Show or set max history messages (-1 = unlimited) [saved]"),
    ("memory", "Show current token usage breakdown"),
    ("snapshot save", "Save current state as named snapshot"),
    ("snapshot load", "Load snapshot by name"),
    ("snapshot list", "List available snapshots"),
    ("quit", "Exit"),
    ("exit", "Exit"),
    ("q", "Exit"),
]


class CommandCompleter(Completer):
    """Completer for / commands."""

    def get_completions(self, document, complete_event):
        text = document.text_before_cursor

        # Only complete if starts with /
        if not text.startswith("/"):
            return

        # Get the text after /
        cmd_text = text[1:].lower()

        for cmd, description in COMMANDS:
            if cmd.startswith(cmd_text):
                # Yield completion (what to insert, how far back to go)
                yield Completion(
                    cmd,
                    start_position=-len(cmd_text),
                    display=f"/{cmd}",
                    display_meta=description,
                )


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

        # Thread safety for output
        self._output_lock = threading.Lock()

        # Cached pre-parsed output snapshot (ensures atomic consistency
        # between _get_output_formatted() and _get_cursor_position())
        self._cached_formatted: Optional[FormattedText] = None
        self._cached_line_count: int = 0
        self._cached_text_version: str = ""

        # Command queue for background processing
        self._command_queue: queue.Queue[Optional[str]] = queue.Queue()

        # Blocking prompt support (for tool approvals)
        self._pending_blocking_prompt: Optional[queue.Queue[str]] = None

        # Worker thread
        self._worker_thread: Optional[threading.Thread] = None
        self._shutdown = False

        # Spinner state for visual feedback during processing
        self._spinner_text: str = ""
        self._spinner_active = False
        self._spinner_frame = 0
        self._spinner_thread: Optional[threading.Thread] = None
        self._spinner_frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]

        # Prompt history (persists across prompts in this session)
        self._history = InMemoryHistory()

        # Input buffer with command completer and history
        self._input_buffer = Buffer(
            name="input",
            multiline=False,
            completer=CommandCompleter(),
            complete_while_typing=True,
            history=self._history,
        )

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
        """Get formatted output text with ANSI color support (thread-safe).

        Returns cached pre-parsed ANSI result to ensure consistency with
        _get_cursor_position() during the same render cycle. This eliminates
        race conditions where text changes between the two method calls.
        """
        with self._output_lock:
            if self._cached_formatted is None:
                # First call or cache invalidated - rebuild
                self._invalidate_output_cache()
            return self._cached_formatted

    def _get_cursor_position(self) -> Point:
        """Get cursor position for scrolling (thread-safe).

        Uses cached line count to ensure consistency with _get_output_formatted()
        during the same render cycle. Both methods read from the same snapshot,
        eliminating race conditions between text updates and rendering.

        prompt_toolkit scrolls the view to make the cursor visible.
        By setting cursor to scroll_offset, we control which line is visible.
        """
        with self._output_lock:
            if self._cached_formatted is None:
                # Cache not initialized - rebuild
                self._invalidate_output_cache()

            # Use cached line count from same snapshot as formatted text
            total_lines = self._cached_line_count

            # Clamp scroll_offset to valid range [0, total_lines - 1]
            # Line indices are 0-based, so max valid index is total_lines - 1
            safe_offset = max(0, min(self._scroll_offset, total_lines - 1))
            return Point(0, safe_offset)

    def _invalidate_output_cache(self) -> None:
        """Invalidate cached ANSI-parsed output (must be called under lock).

        This ensures both _get_output_formatted() and _get_cursor_position()
        return values from the same text snapshot, eliminating race conditions.

        CRITICAL: Must be called with self._output_lock held.
        """
        if not self._output_text:
            self._cached_formatted = FormattedText([])
            self._cached_line_count = 0
            self._cached_text_version = ""
            return

        # Parse ANSI under lock (happens once per text change)
        self._cached_formatted = ANSI(self._output_text)
        self._cached_line_count = self._output_text.count('\n') + 1
        self._cached_text_version = self._output_text

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
                lambda: [("class:help", " Enter=submit | ↑/↓=history | PgUp/PgDn=scroll | Home/End=top/bottom | Ctrl+C=exit")]
            ),
            height=1,
            style="class:help-bar",
        )

        # Stack everything vertically
        body = HSplit([
            output_window,    # Scrollable output (takes remaining space)
            separator,        # Visual separator
            input_row,        # Input area with prompt
            status_bar,       # Status info
            help_bar,         # Help hints
        ])

        # Wrap in FloatContainer to show completion menu
        root = FloatContainer(
            content=body,
            floats=[
                Float(
                    xcursor=True,
                    ycursor=True,
                    content=CompletionsMenu(max_height=10, scroll_offset=1),
                ),
            ],
        )

        self._layout = Layout(root)
        # Focus starts on input
        self._layout.focus(self._input_buffer)

        # Store references for later
        self._output_window = output_window

    def _get_status_formatted(self) -> FormattedText:
        """Get formatted status text with optional spinner."""
        text = self._get_status_text()

        # If spinner is active, show it prominently
        if self._spinner_active and self._spinner_text:
            spinner_char = self._spinner_frames[self._spinner_frame % len(self._spinner_frames)]
            return [
                ("class:spinner", f" {spinner_char} "),
                ("class:spinner-text", f"{self._spinner_text}"),
                ("class:status-text", f"  │  {text}"),
            ]

        return [("class:status-text", f" {text}")]

    def _build_keybindings(self) -> None:
        """Build key bindings."""
        self._kb = KeyBindings()

        # Enter = submit input (but not if completion menu is showing)
        @self._kb.add("enter", filter=~has_completions)
        def handle_enter(event):
            text = self._input_buffer.text.strip()
            if text:
                # Add to history before clearing
                self._history.append_string(text)
                # Clear input
                self._input_buffer.reset()

                # If there's a pending blocking prompt, respond to it
                if self._pending_blocking_prompt is not None:
                    self._pending_blocking_prompt.put(text)
                else:
                    # Queue for background processing (don't exit app!)
                    self._command_queue.put(text)

                # Trigger UI refresh
                event.app.invalidate()
            else:
                # Empty input - if blocking prompt is waiting, show guidance
                if self._pending_blocking_prompt is not None:
                    self.append_output("  (Please type a response and press Enter)")
                    event.app.invalidate()

        # Enter with completions = accept completion (don't submit)
        @self._kb.add("enter", filter=has_completions)
        def handle_enter_completion(event):
            # Accept the current completion
            buff = event.app.current_buffer
            if buff.complete_state:
                buff.complete_state = None
            # Apply the completion but don't submit
            event.current_buffer.complete_state = None

        # Tab = accept completion
        @self._kb.add("tab", filter=has_completions)
        def handle_tab_completion(event):
            buff = event.app.current_buffer
            if buff.complete_state:
                buff.complete_state = None

        # Up arrow = history previous (when no completions showing)
        @self._kb.add("up", filter=~has_completions)
        def history_prev(event):
            event.current_buffer.history_backward()

        # Down arrow = history next (when no completions showing)
        @self._kb.add("down", filter=~has_completions)
        def history_next(event):
            event.current_buffer.history_forward()

        # Up arrow with completions = navigate completions
        @self._kb.add("up", filter=has_completions)
        def completion_prev(event):
            buff = event.app.current_buffer
            if buff.complete_state:
                buff.complete_previous()

        # Down arrow with completions = navigate completions
        @self._kb.add("down", filter=has_completions)
        def completion_next(event):
            buff = event.app.current_buffer
            if buff.complete_state:
                buff.complete_next()

        # Ctrl+C = exit
        @self._kb.add("c-c")
        def handle_ctrl_c(event):
            self._shutdown = True
            self._command_queue.put(None)  # Signal worker to stop
            event.app.exit(result=None)

        # Ctrl+D = exit (EOF)
        @self._kb.add("c-d")
        def handle_ctrl_d(event):
            self._shutdown = True
            self._command_queue.put(None)  # Signal worker to stop
            event.app.exit(result=None)

        # Ctrl+L = clear output
        @self._kb.add("c-l")
        def handle_ctrl_l(event):
            self.clear_output()
            event.app.invalidate()

        # Ctrl+Up = scroll output up
        @self._kb.add("c-up")
        def scroll_up(event):
            self._scroll(-3)
            event.app.invalidate()

        # Ctrl+Down = scroll output down
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

        # Home = scroll to top
        @self._kb.add("home")
        def scroll_to_top(event):
            self._scroll_offset = 0
            event.app.invalidate()

        # End = scroll to bottom
        @self._kb.add("end")
        def scroll_to_end(event):
            total_lines = self._get_total_lines()
            self._scroll_offset = max(0, total_lines - 1)
            event.app.invalidate()

        # Alt+Enter = insert newline in input
        @self._kb.add("escape", "enter")
        def handle_alt_enter(event):
            self._input_buffer.insert_text("\n")

        # Ctrl+J = insert newline (Unix tradition)
        @self._kb.add("c-j")
        def handle_ctrl_j(event):
            self._input_buffer.insert_text("\n")

    def _get_total_lines(self) -> int:
        """Get total number of lines in output (thread-safe)."""
        with self._output_lock:
            if not self._output_text:
                return 0
            return self._output_text.count('\n') + 1

    def _scroll(self, lines: int) -> None:
        """Scroll the output by N lines."""
        total_lines = self._get_total_lines()
        if total_lines == 0:
            return
        # Line indices are 0-based, so valid range is [0, total_lines - 1]
        max_offset = max(0, total_lines - 1)
        self._scroll_offset = max(0, min(max_offset, self._scroll_offset + lines))

    def scroll_to_bottom(self) -> None:
        """Scroll to show the latest content at the bottom."""
        total_lines = self._get_total_lines()
        # Set cursor to last valid line index (0-based)
        self._scroll_offset = max(0, total_lines - 1)
        if self._app and self._app.is_running:
            self._app.invalidate()

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
                # Spinner styling
                "spinner": "#00aaff bold",
                "spinner-text": "#ffaa00",
                # Completion menu styling
                "completion-menu": "bg:#1a1a2e #cccccc",
                "completion-menu.completion": "bg:#1a1a2e #cccccc",
                "completion-menu.completion.current": "bg:#444444 #ffffff bold",
                "completion-menu.meta.completion": "bg:#1a1a2e #888888 italic",
                "completion-menu.meta.completion.current": "bg:#444444 #aaaaaa italic",
            })
        else:
            self._style = Style.from_dict({})

    def append_output(self, text: str) -> None:
        """Append text to the output area (thread-safe)."""
        with self._output_lock:
            if self._output_text:
                self._output_text += "\n" + text
            else:
                self._output_text = text

            # Invalidate cache - pre-parse ANSI under lock to ensure
            # atomic consistency between formatted text and line count
            self._invalidate_output_cache()

            # Auto-scroll to bottom when new content added
            # Use cached line count from the snapshot we just created
            self._scroll_offset = max(0, self._cached_line_count - 1)

        # Trigger UI refresh (now safe - cache updated atomically)
        if self._app and self._app.is_running:
            self._app.invalidate()

    def clear_output(self) -> None:
        """Clear the output area (thread-safe)."""
        with self._output_lock:
            self._output_text = ""
            self._invalidate_output_cache()  # Clear cache atomically
            self._scroll_offset = 0

        if self._app and self._app.is_running:
            self._app.invalidate()

    def set_output(self, text: str) -> None:
        """Replace all output with new text (thread-safe)."""
        with self._output_lock:
            self._output_text = text
            self._invalidate_output_cache()  # Pre-parse under lock
            self._scroll_offset = 0

        if self._app and self._app.is_running:
            self._app.invalidate()

    def _spinner_loop(self) -> None:
        """Background thread that animates the spinner."""
        while self._spinner_active and not self._shutdown:
            self._spinner_frame = (self._spinner_frame + 1) % len(self._spinner_frames)
            if self._app and self._app.is_running:
                self._app.invalidate()
            time.sleep(0.1)  # 10 FPS animation

    def set_spinner(self, text: str) -> None:
        """Start the spinner with the given text (thread-safe).

        Args:
            text: Status text to show next to the spinner (e.g., "Generating...")
        """
        self._spinner_text = text
        self._spinner_frame = 0

        if not self._spinner_active:
            self._spinner_active = True
            self._spinner_thread = threading.Thread(target=self._spinner_loop, daemon=True)
            self._spinner_thread.start()
        elif self._app and self._app.is_running:
            self._app.invalidate()

    def clear_spinner(self) -> None:
        """Stop and hide the spinner (thread-safe)."""
        self._spinner_active = False
        self._spinner_text = ""

        if self._spinner_thread:
            self._spinner_thread.join(timeout=0.5)
            self._spinner_thread = None

        if self._app and self._app.is_running:
            self._app.invalidate()

    def _worker_loop(self) -> None:
        """Background thread that processes commands from the queue."""
        while not self._shutdown:
            try:
                cmd = self._command_queue.get(timeout=0.1)
            except queue.Empty:
                continue

            if cmd is None:  # Shutdown signal
                break

            try:
                self._on_input(cmd)
            except KeyboardInterrupt:
                self.append_output("Interrupted.")
            except Exception as e:
                self.append_output(f"Error: {e}")
            finally:
                # Trigger UI refresh from worker thread (thread-safe)
                if self._app and self._app.is_running:
                    self._app.invalidate()

    def run_loop(self, banner: str = "") -> None:
        """Run the main input loop with single Application lifecycle.

        The Application stays in full-screen mode continuously. Commands are
        processed by a background worker thread while the UI remains responsive.

        Args:
            banner: Initial text to show in output
        """
        if banner:
            self.set_output(banner)

        # Start worker thread
        self._shutdown = False
        self._running = True
        self._worker_thread = threading.Thread(target=self._worker_loop, daemon=True)
        self._worker_thread.start()

        try:
            # Run the app ONCE - stays in full-screen until explicit exit
            self._app.run()
        except (EOFError, KeyboardInterrupt):
            pass
        finally:
            # Clean shutdown
            self._running = False
            self._shutdown = True
            self._command_queue.put(None)
            if self._worker_thread:
                self._worker_thread.join(timeout=2.0)

    def blocking_prompt(self, message: str) -> str:
        """Block worker thread until user provides input (for tool approvals).

        This method is called from the worker thread when tool approval is needed.
        It shows the message in output and waits for the user to respond.

        Args:
            message: The prompt message to show

        Returns:
            The user's response, or empty string on timeout
        """
        self.append_output(message)

        response_queue: queue.Queue[str] = queue.Queue()
        self._pending_blocking_prompt = response_queue

        try:
            return response_queue.get(timeout=300)  # 5 minute timeout
        except queue.Empty:
            return ""
        finally:
            self._pending_blocking_prompt = None

    def stop(self) -> None:
        """Stop the run loop and exit the application."""
        self._running = False
        self._shutdown = True
        self._command_queue.put(None)
        if self._app and self._app.is_running:
            self._app.exit()

    def exit(self) -> None:
        """Exit the application (alias for stop)."""
        self.stop()
