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
from typing import Any, Callable, Dict, List, Optional, Tuple

from prompt_toolkit.application import Application
from prompt_toolkit.application.current import get_app
from prompt_toolkit.buffer import Buffer
from prompt_toolkit.completion import Completer, Completion
from prompt_toolkit.filters import Always, Never, has_completions
from prompt_toolkit.history import InMemoryHistory
from prompt_toolkit.data_structures import Point
from prompt_toolkit.formatted_text import FormattedText, ANSI
from prompt_toolkit.formatted_text.utils import to_formatted_text
from prompt_toolkit.key_binding import KeyBindings
from prompt_toolkit.layout.containers import Float, FloatContainer, HSplit, VSplit, Window
from prompt_toolkit.layout.controls import BufferControl, FormattedTextControl
from prompt_toolkit.layout.layout import Layout
from prompt_toolkit.layout.menus import CompletionsMenu
from prompt_toolkit.mouse_events import MouseEvent, MouseEventType
from prompt_toolkit.styles import Style


# Command definitions: (command, description)
COMMANDS = [
    ("help", "Show available commands"),
    ("tools", "List available tools"),
    ("status", "Show current run status"),
    ("history", "Show recent conversation history"),
    ("copy", "Copy messages to clipboard (/copy user|assistant [turn])"),
    ("resume", "Resume the saved/attached run"),
    ("clear", "Clear memory and start fresh"),
    ("compact", "Compress conversation [light|standard|heavy] [--preserve N] [focus...]"),
    ("spans", "List archived conversation spans (from /compact)"),
    ("expand", "Expand an archived span into view/context"),
    ("recall", "Recall memory spans by query/time/tags"),
    ("vars", "Inspect durable run vars (scratchpad, _runtime, ...)"),
    ("remember", "Store a durable memory note"),
    ("flow", "Run AbstractFlow workflows (run/resume/pause/cancel)"),
    ("mouse", "Toggle mouse mode (wheel scroll vs terminal selection)"),
    ("new", "Start fresh (alias for /clear)"),
    ("reset", "Reset session (alias for /clear)"),
    ("task", "Start a new task (/task <text>)"),
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

    _COPY_MARKER_RE = re.compile(r"\[\[COPY:([^\]]+)\]\]")

    class _ScrollAwareFormattedTextControl(FormattedTextControl):
        def __init__(
            self,
            *,
            text: Callable[[], FormattedText],
            get_cursor_position: Callable[[], Point],
            on_scroll: Callable[[int], None],
        ):
            super().__init__(
                text=text,
                focusable=True,
                get_cursor_position=get_cursor_position,
            )
            self._on_scroll = on_scroll

        def mouse_handler(self, mouse_event: MouseEvent):  # type: ignore[override]
            if mouse_event.event_type == MouseEventType.SCROLL_UP:
                self._on_scroll(-1)
                return None
            if mouse_event.event_type == MouseEventType.SCROLL_DOWN:
                self._on_scroll(1)
                return None
            return super().mouse_handler(mouse_event)

    def __init__(
        self,
        get_status_text: Callable[[], str],
        on_input: Callable[[str], None],
        on_copy_payload: Optional[Callable[[str], bool]] = None,
        color: bool = True,
        mouse_support: bool = True,
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
        self._mouse_support_enabled = bool(mouse_support)
        self._running = False

        self._on_copy_payload = on_copy_payload
        self._copy_payloads: Dict[str, str] = {}

        # Output content storage (raw text with ANSI codes)
        self._output_text: str = ""
        # Always track at least 1 line (even when output is empty).
        self._output_line_count: int = 1
        # Scroll position (line offset from top)
        self._scroll_offset: int = 0
        # When True, keep the view pinned to the latest output.
        # When the user scrolls up, this is disabled until they scroll back to bottom.
        self._follow_output: bool = True

        # Thread safety for output
        self._output_lock = threading.Lock()

        # Render-cycle cache: keep output stable during a single render pass.
        # This prevents prompt_toolkit from seeing text/cursor from different snapshots.
        self._render_cache_counter: Optional[int] = None
        self._render_cache_formatted: FormattedText = ANSI("")
        self._render_cache_line_count: int = 1

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
            mouse_support=self._mouse_support_enabled,
            erase_when_done=False,
        )

    def register_copy_payload(self, copy_id: str, payload: str) -> None:
        """Register a payload for a clickable [[COPY:...]] marker in the output."""
        cid = str(copy_id or "").strip()
        if not cid:
            return
        with self._output_lock:
            self._copy_payloads[cid] = str(payload or "")

    def toggle_mouse_support(self) -> bool:
        """Toggle mouse reporting (wheel scroll) vs terminal selection mode."""
        self._mouse_support_enabled = not self._mouse_support_enabled
        try:
            # prompt_toolkit prefers Filter objects for runtime toggling.
            self._app.mouse_support = Always() if self._mouse_support_enabled else Never()  # type: ignore[assignment]
        except Exception:
            try:
                self._app.mouse_support = self._mouse_support_enabled  # type: ignore[assignment]
            except Exception:
                pass
        try:
            if self._mouse_support_enabled:
                self._app.output.enable_mouse_support()
            else:
                self._app.output.disable_mouse_support()
            self._app.output.flush()
        except Exception:
            pass
        if self._app and self._app.is_running:
            self._app.invalidate()
        return self._mouse_support_enabled

    def _copy_handler(self, copy_id: str) -> Callable[[MouseEvent], None]:
        def _handler(mouse_event: MouseEvent) -> None:
            if mouse_event.event_type not in (MouseEventType.MOUSE_UP, MouseEventType.MOUSE_DOWN):
                return
            if self._on_copy_payload is None:
                return
            with self._output_lock:
                payload = self._copy_payloads.get(copy_id)
            if payload is None:
                return
            try:
                self._on_copy_payload(payload)
            except Exception:
                return

        return _handler

    def _format_output_text(self, text: str) -> FormattedText:
        """Convert output text into formatted fragments and attach handlers for copy markers."""
        if not text:
            return to_formatted_text(ANSI(""))

        if "[[COPY:" not in text:
            return to_formatted_text(ANSI(text))

        out: List[Tuple[Any, ...]] = []
        pos = 0
        for m in self._COPY_MARKER_RE.finditer(text):
            before = text[pos : m.start()]
            if before:
                out.extend(to_formatted_text(ANSI(before)))
            copy_id = str(m.group(1) or "").strip()
            if copy_id:
                out.append(("class:copy-button", "[ copy ]", self._copy_handler(copy_id)))
            else:
                out.extend(to_formatted_text(ANSI(m.group(0))))
            pos = m.end()
        tail = text[pos:]
        if tail:
            out.extend(to_formatted_text(ANSI(tail)))
        return out

    def _ensure_render_cache(self) -> None:
        """Freeze a per-render snapshot for prompt_toolkit.

        prompt_toolkit may call our text provider and cursor provider multiple times
        in one render pass. If output changes between those calls, prompt_toolkit can
        crash (e.g. while wrapping/scrolling). We avoid that by caching a snapshot
        keyed by `Application.render_counter`.
        """
        try:
            render_counter = get_app().render_counter
        except Exception:
            render_counter = None

        with self._output_lock:
            if self._render_cache_counter == render_counter:
                return
            text_snapshot = self._output_text
            line_count_snapshot = self._output_line_count

        formatted = self._format_output_text(text_snapshot)
        line_count = max(1, int(line_count_snapshot or 1))

        with self._output_lock:
            # Don't overwrite a cache that was already created for this render.
            if self._render_cache_counter == render_counter:
                return
            self._render_cache_counter = render_counter
            self._render_cache_formatted = formatted
            self._render_cache_line_count = line_count

    def _get_output_formatted(self) -> FormattedText:
        """Get formatted output text with ANSI color support (render-stable)."""
        self._ensure_render_cache()
        with self._output_lock:
            return self._render_cache_formatted

    def _get_cursor_position(self) -> Point:
        """Get cursor position for scrolling (render-stable)."""
        self._ensure_render_cache()
        with self._output_lock:
            total_lines = self._render_cache_line_count
            safe_offset = max(0, min(self._scroll_offset, total_lines - 1))
            return Point(0, safe_offset)

    def _build_layout(self) -> None:
        """Build the HSplit layout with output, input, and status areas."""
        # Output area using FormattedTextControl for ANSI color support
        self._output_control = self._ScrollAwareFormattedTextControl(
            text=self._get_output_formatted,
            get_cursor_position=self._get_cursor_position,
            on_scroll=self._scroll,
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
                lambda: [("class:help", " Enter=submit | ↑/↓=history | Ctrl+↑/↓ or Wheel=scroll | Home=top | End=follow | Ctrl+C=exit")]
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

                # After submitting, jump back to the latest output.
                self.scroll_to_bottom()

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

        # Ctrl+Down = scroll output down
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

        # Shift+PageUp/PageDown (some terminals send these for paging)
        @self._kb.add("s-pageup")
        def shift_page_up(event):
            self._scroll(-10)

        @self._kb.add("s-pagedown")
        def shift_page_down(event):
            self._scroll(10)

        # Mouse wheel scroll (trackpad / wheel)
        @self._kb.add("<scroll-up>")
        def mouse_scroll_up(event):
            self._scroll(-1)

        @self._kb.add("<scroll-down>")
        def mouse_scroll_down(event):
            self._scroll(1)

        # Home = scroll to top
        @self._kb.add("home")
        def scroll_to_top(event):
            self._scroll_offset = 0
            self._follow_output = False
            event.app.invalidate()

        # End = scroll to bottom
        @self._kb.add("end")
        def scroll_to_end(event):
            self.scroll_to_bottom()

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
            return self._output_line_count

    def _scroll(self, lines: int) -> None:
        """Scroll the output by N lines."""
        with self._output_lock:
            total_lines = self._output_line_count
            # Line indices are 0-based, so valid range is [0, total_lines - 1]
            max_offset = max(0, total_lines - 1)
            self._scroll_offset = max(0, min(max_offset, self._scroll_offset + lines))

            # User-initiated scroll disables follow mode until we return to bottom.
            if lines < 0:
                self._follow_output = False
            elif lines > 0 and self._scroll_offset >= max_offset:
                self._follow_output = True
        if self._app and self._app.is_running:
            self._app.invalidate()

    def scroll_to_bottom(self) -> None:
        """Scroll to show the latest content at the bottom."""
        with self._output_lock:
            total_lines = self._output_line_count
            self._scroll_offset = max(0, total_lines - 1)
            self._follow_output = True
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
                "copy-button": "bg:#333333 #ffffff",
            })
        else:
            self._style = Style.from_dict({})

    def append_output(self, text: str) -> None:
        """Append text to the output area (thread-safe)."""
        with self._output_lock:
            if self._output_text:
                self._output_text += "\n" + text
                self._output_line_count += 1 + text.count("\n")
            else:
                self._output_text = text
                self._output_line_count = max(1, text.count("\n") + 1)

            # Auto-scroll to bottom only when following output.
            if self._follow_output:
                self._scroll_offset = max(0, self._output_line_count - 1)
            else:
                # Keep current view, but make sure it's still a valid offset.
                self._scroll_offset = max(0, min(self._scroll_offset, self._output_line_count - 1))

        # Trigger UI refresh (now safe - cache updated atomically)
        if self._app and self._app.is_running:
            self._app.invalidate()

    def clear_output(self) -> None:
        """Clear the output area (thread-safe)."""
        with self._output_lock:
            self._output_text = ""
            self._output_line_count = 1
            self._scroll_offset = 0
            self._follow_output = True
            self._copy_payloads.clear()

        if self._app and self._app.is_running:
            self._app.invalidate()

    def set_output(self, text: str) -> None:
        """Replace all output with new text (thread-safe)."""
        with self._output_lock:
            self._output_text = text
            self._output_line_count = max(1, text.count("\n") + 1) if text else 1
            self._scroll_offset = 0
            self._follow_output = True
            self._copy_payloads.clear()

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
        # Tool approvals must be visible even if the user scrolled up.
        self.scroll_to_bottom()
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
