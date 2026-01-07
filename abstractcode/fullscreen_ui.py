"""Full-screen UI with scrollable history, fixed input, and status bar.

Uses prompt_toolkit's Application with HSplit layout to provide:
- Scrollable output/history area (mouse wheel + keyboard) with ANSI color support
- Fixed input area at bottom
- Fixed status bar showing provider/model/context info
- Command autocomplete when typing /
"""

from __future__ import annotations

from dataclasses import dataclass
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
    ("plan", "Toggle Plan mode (TODO list first) [saved]"),
    ("review", "Toggle Review mode (self-check) [saved]"),
    ("resume", "Resume the saved/attached run"),
    ("pause", "Pause the current run (durable)"),
    ("cancel", "Cancel the current run (durable)"),
    ("clear", "Clear memory and clear the screen"),
    ("compact", "Compress conversation [light|standard|heavy] [--preserve N] [focus...]"),
    ("spans", "List archived conversation spans (from /compact)"),
    ("expand", "Expand an archived span into view/context"),
    ("recall", "Recall memory spans by query/time/tags"),
    ("vars", "Inspect durable run vars (scratchpad, _runtime, ...)"),
    ("context", "Show the exact context for the next LLM call"),
    ("memorize", "Store a durable memory note"),
    ("flow", "Run AbstractFlow workflows (run/resume/pause/cancel)"),
    ("mouse", "Toggle mouse mode (wheel scroll vs terminal selection)"),
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

    _MARKER_RE = re.compile(r"\[\[(COPY|SPINNER|FOLD):([^\]]+)\]\]")

    @dataclass
    class _FoldRegion:
        """A collapsible region rendered inline in the scrollback.

        - `visible_lines` are always displayed.
        - `hidden_lines` are displayed only when expanded.
        - `start_idx` is the absolute line index (in `_output_lines`) of the first visible line.
        """

        fold_id: str
        start_idx: int
        visible_lines: List[str]
        hidden_lines: List[str]
        collapsed: bool = True

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
        on_fold_toggle: Optional[Callable[[str], None]] = None,
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

        self._on_fold_toggle = on_fold_toggle
        self._fold_regions: Dict[str, FullScreenUI._FoldRegion] = {}

        # Output content storage (raw text lines with ANSI codes).
        # Keeping a line list lets us render a virtualized view window instead of
        # re-wrapping the entire history every frame.
        self._output_lines: List[str] = [""]
        # Always track at least 1 line (even when output is empty).
        self._output_line_count: int = 1
        # Monotonic counter incremented whenever output text changes.
        # Used to cache expensive ANSI/marker parsing across renders.
        self._output_version: int = 0
        # Scroll position (line offset from top)
        self._scroll_offset: int = 0
        # Cursor column within the current line. This matters for wrapped lines:
        # prompt_toolkit uses the cursor column to scroll within a long wrapped line.
        self._scroll_col: int = 0
        # When True, keep the view pinned to the latest output.
        # When the user scrolls up, this is disabled until they scroll back to bottom.
        self._follow_output: bool = True
        # Mouse wheel events can arrive in rapid bursts (especially on high-resolution wheels).
        # Reduce perceived scroll speed by dropping ~30% of wheel ticks (Bresenham-style).
        self._wheel_scroll_skip_accum: int = 0
        self._wheel_scroll_skip_numerator: int = 3
        self._wheel_scroll_skip_denominator: int = 10

        # Virtualized view window in absolute line indices [start, end).
        # Only this window is rendered by prompt_toolkit for performance.
        self._view_start: int = 0
        self._view_end: int = 1
        self._last_output_window_height: int = 0

        # Thread safety for output
        self._output_lock = threading.Lock()

        # Render-cycle cache: keep output stable during a single render pass.
        # This prevents prompt_toolkit from seeing text/cursor from different snapshots.
        self._render_cache_counter: Optional[int] = None
        self._render_cache_formatted: FormattedText = ANSI("")
        self._render_cache_line_count: int = 1
        # Cross-render cache: only reformat output when output text changes.
        self._formatted_cache_key: Optional[Tuple[int, int, int]] = None
        self._formatted_cache_formatted: FormattedText = ANSI("")
        self._render_cache_view_start: int = 0
        self._render_cache_cursor_row: int = 0
        self._render_cache_cursor_col: int = 0

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
        # Optional auto-clear timer for transient status messages.
        self._spinner_token: int = 0
        self._spinner_clear_timer: Optional[threading.Timer] = None

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

    def replace_output_marker(self, marker: str, replacement: str) -> bool:
        """Replace the first occurrence of `marker` in the output with `replacement`.

        This is used for lightweight in-place updates (e.g., tool-line spinners → ✅/❌)
        without requiring a full structured output model.
        """
        needle = str(marker or "")
        if not needle:
            return False
        repl = str(replacement or "")
        with self._output_lock:
            # Search from the end: markers are almost always near the latest output.
            for i in range(len(self._output_lines) - 1, -1, -1):
                line = self._output_lines[i]
                if needle not in line:
                    continue
                self._output_lines[i] = line.replace(needle, repl, 1)
                # Line count unchanged (marker + replacement should not contain newlines).
                self._output_version += 1
                break
            else:
                return False
        if self._app and self._app.is_running:
            self._app.invalidate()
        return True

    def append_fold_region(
        self,
        *,
        fold_id: str,
        visible_lines: List[str],
        hidden_lines: List[str],
        collapsed: bool = True,
    ) -> None:
        """Append a collapsible region to the output.

        The region is addressable via `[[FOLD:<fold_id>]]` markers embedded in `visible_lines`.
        """
        fid = str(fold_id or "").strip()
        if not fid:
            return
        vis = list(visible_lines or [])
        hid = list(hidden_lines or [])
        if not vis:
            vis = [f"[[FOLD:{fid}]]"]

        with self._output_lock:
            start_idx = len(self._output_lines)
            self._output_lines.extend(vis)
            if not collapsed and hid:
                self._output_lines.extend(hid)
            self._output_line_count = max(1, len(self._output_lines))
            self._output_version += 1

            self._fold_regions[fid] = FullScreenUI._FoldRegion(
                fold_id=fid,
                start_idx=start_idx,
                visible_lines=vis,
                hidden_lines=hid,
                collapsed=bool(collapsed),
            )

            if self._follow_output:
                self._scroll_offset = max(0, self._output_line_count - 1)
                self._scroll_col = 10**9
            else:
                self._scroll_offset = max(0, min(self._scroll_offset, self._output_line_count - 1))
                self._scroll_col = max(0, int(self._scroll_col or 0))
            self._ensure_view_window_locked()

        if self._app and self._app.is_running:
            self._app.invalidate()

    def update_fold_region(
        self,
        fold_id: str,
        *,
        visible_lines: Optional[List[str]] = None,
        hidden_lines: Optional[List[str]] = None,
    ) -> bool:
        """Update an existing fold region in-place.

        If the region is expanded and `hidden_lines` changes length, subsequent fold regions are shifted.
        """
        fid = str(fold_id or "").strip()
        if not fid:
            return False

        def _shift_regions(after_idx: int, delta: int, *, exclude: str) -> None:
            if not delta:
                return
            for rid, reg in self._fold_regions.items():
                if rid == exclude:
                    continue
                if reg.start_idx >= after_idx:
                    reg.start_idx += delta

        with self._output_lock:
            reg = self._fold_regions.get(fid)
            if reg is None:
                return False

            vis_old = list(reg.visible_lines)
            hid_old = list(reg.hidden_lines)
            vis_new = list(visible_lines) if visible_lines is not None else vis_old
            hid_new = list(hidden_lines) if hidden_lines is not None else hid_old

            # Compute where the region is currently rendered in `_output_lines`.
            start = int(reg.start_idx)
            if start < 0:
                start = 0
            # Best-effort safety if output was cleared externally.
            if start >= len(self._output_lines):
                return False

            current_len = len(vis_old) + (0 if reg.collapsed else len(hid_old))
            new_len = len(vis_new) + (0 if reg.collapsed else len(hid_new))

            # Replace the rendered slice.
            end = min(len(self._output_lines), start + current_len)
            rendered = list(vis_new)
            if not reg.collapsed:
                rendered.extend(hid_new)
            self._output_lines[start:end] = rendered

            delta = new_len - current_len
            if delta:
                _shift_regions(after_idx=start + current_len, delta=delta, exclude=fid)

            reg.visible_lines = vis_new
            reg.hidden_lines = hid_new

            self._output_line_count = max(1, len(self._output_lines))
            self._output_version += 1
            self._scroll_offset = max(0, min(self._scroll_offset, self._output_line_count - 1))
            self._ensure_view_window_locked()

        if self._app and self._app.is_running:
            self._app.invalidate()
        return True

    def toggle_fold(self, fold_id: str) -> bool:
        """Toggle a fold region (collapsed/expanded) by id."""
        fid = str(fold_id or "").strip()
        if not fid:
            return False

        def _shift_regions(after_idx: int, delta: int, *, exclude: str) -> None:
            if not delta:
                return
            for rid, reg in self._fold_regions.items():
                if rid == exclude:
                    continue
                if reg.start_idx >= after_idx:
                    reg.start_idx += delta

        with self._output_lock:
            reg = self._fold_regions.get(fid)
            if reg is None:
                return False

            start = int(reg.start_idx)
            start = max(0, min(start, max(0, len(self._output_lines) - 1)))
            insert_at = start + len(reg.visible_lines)

            if reg.collapsed:
                # Expand: insert hidden lines.
                if reg.hidden_lines:
                    self._output_lines[insert_at:insert_at] = list(reg.hidden_lines)
                    _shift_regions(after_idx=insert_at, delta=len(reg.hidden_lines), exclude=fid)
                reg.collapsed = False
            else:
                # Collapse: remove hidden lines slice.
                n = len(reg.hidden_lines)
                if n:
                    del self._output_lines[insert_at : insert_at + n]
                    _shift_regions(after_idx=insert_at, delta=-n, exclude=fid)
                reg.collapsed = True

            self._output_line_count = max(1, len(self._output_lines))
            self._output_version += 1
            self._scroll_offset = max(0, min(self._scroll_offset, self._output_line_count - 1))
            self._ensure_view_window_locked()

        if self._app and self._app.is_running:
            self._app.invalidate()
        return True

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

    def _fold_handler(self, fold_id: str) -> Callable[[MouseEvent], None]:
        def _handler(mouse_event: MouseEvent) -> None:
            # Important: only toggle on MOUSE_UP.
            # prompt_toolkit typically emits both DOWN and UP for a click; toggling on both
            # will expand then immediately collapse (the "briefly unfolds then snaps back" bug).
            if mouse_event.event_type != MouseEventType.MOUSE_UP:
                return
            fid = str(fold_id or "").strip()
            if not fid:
                return
            # Host callback (optional): lets outer layers synchronize additional state.
            try:
                if self._on_fold_toggle is not None:
                    self._on_fold_toggle(fid)
            except Exception:
                pass
            # Always toggle locally for immediate UX.
            try:
                self.toggle_fold(fid)
            except Exception:
                return

        return _handler

    def _format_output_text(self, text: str) -> FormattedText:
        """Convert output text into formatted fragments and attach handlers for copy markers."""
        if not text:
            return to_formatted_text(ANSI(""))

        if "[[" not in text:
            return to_formatted_text(ANSI(text))

        def _attach_handler_until_newline(
            fragments: FormattedText, handler: Callable[[MouseEvent], None]
        ) -> tuple[FormattedText, bool]:
            """Attach a mouse handler to fragments until the next newline.

            Returns (new_fragments, still_active), where still_active is True iff no newline
            was encountered (so caller should keep the handler for subsequent fragments).
            """
            out_frags: List[Tuple[Any, ...]] = []
            active = True
            for frag in fragments:
                # frag can be (style, text) or (style, text, handler)
                if len(frag) < 2:
                    out_frags.append(frag)
                    continue
                style = frag[0]
                s = frag[1]
                existing_handler = frag[2] if len(frag) >= 3 else None
                if not active or not isinstance(s, str) or "\n" not in s:
                    if active and existing_handler is None:
                        out_frags.append((style, s, handler))
                    else:
                        out_frags.append(frag)
                    continue

                # Split on the first newline: handler applies only before it.
                before, after = s.split("\n", 1)
                if before:
                    if existing_handler is None:
                        out_frags.append((style, before, handler))
                    else:
                        out_frags.append((style, before, existing_handler))
                out_frags.append((style, "\n"))
                if after:
                    out_frags.append((style, after))
                active = False

            return out_frags, active

        out: List[Tuple[Any, ...]] = []
        pos = 0
        active_fold_handler: Optional[Callable[[MouseEvent], None]] = None
        for m in self._MARKER_RE.finditer(text):
            before = text[pos : m.start()]
            if before:
                before_frags = to_formatted_text(ANSI(before))
                if active_fold_handler is not None:
                    patched, still_active = _attach_handler_until_newline(before_frags, active_fold_handler)
                    out.extend(patched)
                    if not still_active:
                        active_fold_handler = None
                else:
                    out.extend(before_frags)
            kind = str(m.group(1) or "").strip().upper()
            payload = str(m.group(2) or "").strip()
            if kind == "COPY":
                if payload:
                    out.append(("class:copy-button", "[ copy ]", self._copy_handler(payload)))
                else:
                    out.extend(to_formatted_text(ANSI(m.group(0))))
            elif kind == "SPINNER":
                if payload:
                    # Keep inline spinners static; the status bar already animates.
                    # This avoids reformatting the whole history on every spinner frame.
                    out.append(("class:inline-spinner", "…"))
                else:
                    out.extend(to_formatted_text(ANSI(m.group(0))))
            elif kind == "FOLD":
                if payload:
                    collapsed = True
                    with self._output_lock:
                        reg = self._fold_regions.get(payload)
                        if reg is not None:
                            collapsed = bool(reg.collapsed)
                    arrow = "▶" if collapsed else "▼"
                    handler = self._fold_handler(payload)
                    # Make the whole header line clickable by attaching this handler to
                    # subsequent fragments until the next newline.
                    out.append(("class:fold-toggle", f"{arrow} ", handler))
                    active_fold_handler = handler
                else:
                    out.extend(to_formatted_text(ANSI(m.group(0))))
            else:
                out.extend(to_formatted_text(ANSI(m.group(0))))
            pos = m.end()
        tail = text[pos:]
        if tail:
            tail_frags = to_formatted_text(ANSI(tail))
            if active_fold_handler is not None:
                patched, _still_active = _attach_handler_until_newline(tail_frags, active_fold_handler)
                out.extend(patched)
            else:
                out.extend(tail_frags)
        return out

    def _compute_view_params_locked(self) -> Tuple[int, int]:
        """Compute (view_size_lines, margin_lines) for output virtualization."""
        height = int(self._last_output_window_height or 0)
        if height <= 0:
            height = 40

        # Heuristic: keep a few dozen screens worth of lines around the cursor.
        # This makes wheel scrolling smooth while avoiding O(total_history) rendering.
        view_size = max(400, height * 25)
        margin = max(100, height * 8)
        margin = min(margin, max(1, view_size // 3))
        return view_size, margin

    def _ensure_view_window_locked(self) -> None:
        """Ensure the virtualized view window includes the current cursor line."""
        if not self._output_lines:
            self._output_lines = [""]
            self._output_line_count = 1

        total_lines = len(self._output_lines)
        self._output_line_count = max(1, total_lines)

        cursor = int(self._scroll_offset or 0)
        cursor = max(0, min(cursor, total_lines - 1))
        self._scroll_offset = cursor

        view_size, margin = self._compute_view_params_locked()
        view_size = max(1, min(int(view_size), total_lines))
        margin = max(0, min(int(margin), max(0, view_size - 1)))

        max_start = max(0, total_lines - view_size)
        start = int(self._view_start or 0)
        end = int(self._view_end or 0)

        window_size_ok = (end - start) == view_size and 0 <= start <= end <= total_lines
        if not window_size_ok:
            start = max(0, min(max_start, cursor - margin))
            end = start + view_size
        else:
            if cursor < start + margin:
                start = max(0, min(max_start, cursor - margin))
                end = start + view_size
            elif cursor > end - margin - 1:
                start = cursor - (view_size - margin - 1)
                start = max(0, min(max_start, start))
                end = start + view_size

        self._view_start = int(start)
        self._view_end = int(min(total_lines, max(start + 1, end)))

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

        # Capture output window height (used for virtualized buffer sizing).
        window_height = 0
        try:
            info = getattr(self, "_output_window", None)
            render_info = getattr(info, "render_info", None) if info is not None else None
            window_height = int(getattr(render_info, "window_height", 0) or 0)
        except Exception:
            window_height = 0

        with self._output_lock:
            if render_counter is not None and self._render_cache_counter == render_counter:
                return
            version_snapshot = self._output_version
            if window_height > 0:
                self._last_output_window_height = window_height

            self._ensure_view_window_locked()
            view_start = int(self._view_start)
            view_end = int(self._view_end)

            view_lines = list(self._output_lines[view_start:view_end])
            view_line_count = max(1, len(view_lines))

            cursor_row_abs = int(self._scroll_offset or 0)
            cursor_row = max(0, min(view_line_count - 1, cursor_row_abs - view_start))
            cursor_col = max(0, int(self._scroll_col or 0))

            cache_key = (int(version_snapshot), int(view_start), int(view_end))
            cached = self._formatted_cache_formatted if self._formatted_cache_key == cache_key else None

        view_text = "\n".join(view_lines)
        formatted = cached if cached is not None else self._format_output_text(view_text)

        with self._output_lock:
            if self._formatted_cache_key != cache_key:
                self._formatted_cache_key = cache_key
                self._formatted_cache_formatted = formatted

            # Don't overwrite a cache that was already created for this render.
            if render_counter is not None and self._render_cache_counter == render_counter:
                return

            self._render_cache_counter = render_counter
            self._render_cache_formatted = formatted
            self._render_cache_line_count = view_line_count
            self._render_cache_view_start = view_start
            self._render_cache_cursor_row = cursor_row
            self._render_cache_cursor_col = cursor_col

    def _get_output_formatted(self) -> FormattedText:
        """Get formatted output text with ANSI color support (render-stable)."""
        self._ensure_render_cache()
        with self._output_lock:
            return self._render_cache_formatted

    def _get_cursor_position(self) -> Point:
        """Get cursor position for scrolling (render-stable)."""
        self._ensure_render_cache()
        with self._output_lock:
            safe_row = max(0, min(int(self._render_cache_cursor_row), int(self._render_cache_line_count) - 1))
            safe_col = max(0, int(self._render_cache_cursor_col or 0))
            return Point(safe_col, safe_row)

    def _scroll_wheel(self, ticks: int) -> None:
        """Scroll handler for mouse wheel events (30% slower)."""
        if not ticks:
            return

        with self._output_lock:
            self._wheel_scroll_skip_accum += int(self._wheel_scroll_skip_numerator)
            if self._wheel_scroll_skip_accum >= int(self._wheel_scroll_skip_denominator):
                self._wheel_scroll_skip_accum -= int(self._wheel_scroll_skip_denominator)
                return

        self._scroll(ticks)

    def _build_layout(self) -> None:
        """Build the HSplit layout with output, input, and status areas."""
        # Output area using FormattedTextControl for ANSI color support
        self._output_control = self._ScrollAwareFormattedTextControl(
            text=self._get_output_formatted,
            get_cursor_position=self._get_cursor_position,
            on_scroll=self._scroll_wheel,
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
            shimmer = str(self._spinner_text or "")
            # "Reflect" shimmer: highlight one character that moves across the text.
            # This is intentionally subtle; the spinner glyph already provides motion.
            parts: List[Tuple[str, str]] = []
            if shimmer:
                # Avoid highlighting whitespace (looks like "no shimmer"). Only sweep over
                # visible characters; highlight a small 3-char window.
                visible_positions = [idx for idx, ch in enumerate(shimmer) if not ch.isspace()]
                if not visible_positions:
                    visible_positions = list(range(len(shimmer)))
                center = visible_positions[int(self._spinner_frame) % max(1, len(visible_positions))]
                lo = max(0, center - 1)
                hi = min(len(shimmer), center + 2)
                pre = shimmer[:lo]
                mid = shimmer[lo:hi]
                post = shimmer[hi:]
                if pre:
                    parts.append(("class:spinner-text", pre))
                if mid:
                    parts.append(("class:spinner-text-highlight", mid))
                if post:
                    parts.append(("class:spinner-text", post))
            text_parts: List[Tuple[str, str]] = parts if parts else [("class:spinner-text", f"{self._spinner_text}")]
            return [
                ("class:spinner", f" {spinner_char} "),
                *text_parts,
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
            self._scroll_wheel(-1)

        @self._kb.add("<scroll-down>")
        def mouse_scroll_down(event):
            self._scroll_wheel(1)

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
        # prompt_toolkit scrolls based on the cursor position. If we increment the
        # cursor by 1 line, the viewport won't move until that cursor hits the edge
        # of the window (cursor-like scrolling). For chat history, wheel scrolling
        # should move the viewport immediately in both directions.
        #
        # To achieve that, we scroll relative to what's currently visible:
        # - scroll up: move the cursor above the first visible line
        # - scroll down: move the cursor below the last visible line
        #
        # That forces prompt_toolkit's Window to adjust vertical_scroll each tick.
        info = getattr(self, "_output_window", None)
        render_info = getattr(info, "render_info", None) if info is not None else None

        with self._output_lock:
            total_lines = self._output_line_count
            # Line indices are 0-based, so valid range is [0, total_lines - 1]
            max_offset = max(0, total_lines - 1)
            view_start = int(self._view_start)
            view_end = int(self._view_end)
            view_line_count = max(1, view_end - view_start)

            # If we're currently on a line that wraps to more rows than the window can show,
            # scroll *within* that line by shifting the cursor column. prompt_toolkit will
            # adjust vertical_scroll_2 accordingly. This avoids the "scroll works only one
            # direction" feeling when a single long line occupies the whole viewport.
            if (
                render_info is not None
                and getattr(render_info, "wrap_lines", False)
                and getattr(render_info, "window_width", 0) > 0
                and getattr(render_info, "window_height", 0) > 0
            ):
                ui_content = getattr(render_info, "ui_content", None)
                width = int(getattr(render_info, "window_width", 0) or 0)
                height = int(getattr(render_info, "window_height", 0) or 0)
                get_line_prefix = getattr(info, "get_line_prefix", None) if info is not None else None

                if ui_content is not None and width > 0 and height > 0:
                    # UIContent line indices are relative to the currently rendered view window.
                    local_line = int(self._scroll_offset) - view_start
                    local_line = max(0, min(local_line, view_line_count - 1))
                    try:
                        line_height = int(
                            ui_content.get_height_for_line(
                                local_line,
                                width,
                                get_line_prefix,
                            )
                        )
                    except Exception:
                        line_height = 0

                    if line_height > height:
                        step = max(1, width)
                        for _ in range(abs(int(lines or 0))):
                            if lines < 0:
                                if self._scroll_col > 0:
                                    self._scroll_col = max(0, int(self._scroll_col) - step)
                                elif self._scroll_offset > 0:
                                    self._scroll_offset = max(0, int(self._scroll_offset) - 1)
                                    # Jump to end-of-line for the previous line so the user can
                                    # scroll upward naturally from the bottom of that line.
                                    self._scroll_col = 10**9
                                self._follow_output = False
                            elif lines > 0:
                                # If we're already at the end of this wrapped line, move to the next line.
                                try:
                                    local_line = int(self._scroll_offset) - view_start
                                    local_line = max(0, min(local_line, view_line_count - 1))
                                    cursor_row = int(
                                        ui_content.get_height_for_line(
                                            local_line,
                                            width,
                                            get_line_prefix,
                                            slice_stop=int(self._scroll_col),
                                        )
                                    )
                                except Exception:
                                    cursor_row = 0

                                if cursor_row >= line_height and self._scroll_offset < max_offset:
                                    self._scroll_offset = min(max_offset, int(self._scroll_offset) + 1)
                                    self._scroll_col = 0
                                else:
                                    self._scroll_col = max(0, int(self._scroll_col) + step)

                        # Clamp and follow-mode update.
                        self._scroll_offset = max(0, min(max_offset, int(self._scroll_offset)))
                        if lines > 0 and self._scroll_offset >= max_offset:
                            try:
                                local_last = int(max_offset) - view_start
                                local_last = max(0, min(local_last, view_line_count - 1))
                                last_height = int(
                                    ui_content.get_height_for_line(
                                        local_last,
                                        width,
                                        get_line_prefix,
                                    )
                                )
                                last_row = int(
                                    ui_content.get_height_for_line(
                                        local_last,
                                        width,
                                        get_line_prefix,
                                        slice_stop=int(self._scroll_col),
                                    )
                                )
                                self._follow_output = last_row >= last_height
                            except Exception:
                                self._follow_output = True
                        self._ensure_view_window_locked()
                        return

            base = self._scroll_offset
            if render_info is not None and getattr(render_info, "content_height", 0) > 0:
                try:
                    if lines < 0:
                        # Use the currently visible region as the baseline, but
                        # allow accumulating scroll ticks before the next render
                        # updates `render_info`.
                        visible_first_local = int(render_info.first_visible_line(after_scroll_offset=True))
                        visible_first = int(view_start) + max(0, visible_first_local)
                        base = min(int(self._scroll_offset), visible_first)
                    else:
                        visible_last_local = int(render_info.last_visible_line(before_scroll_offset=True))
                        visible_last = int(view_start) + max(0, visible_last_local)
                        base = max(int(self._scroll_offset), visible_last)
                except Exception:
                    base = self._scroll_offset

            self._scroll_offset = max(0, min(max_offset, base + lines))
            # When scrolling between lines (not inside a wrapped line), keep the cursor at column 0.
            self._scroll_col = 0

            # User-initiated scroll disables follow mode until we return to bottom.
            if lines < 0:
                self._follow_output = False
            elif lines > 0 and self._scroll_offset >= max_offset:
                self._follow_output = True
            self._ensure_view_window_locked()
        if self._app and self._app.is_running:
            self._app.invalidate()

    def scroll_to_bottom(self) -> None:
        """Scroll to show the latest content at the bottom."""
        with self._output_lock:
            total_lines = self._output_line_count
            self._scroll_offset = max(0, total_lines - 1)
            # Prefer end-of-line so wrapped last lines show their bottom.
            self._scroll_col = 10**9
            self._follow_output = True
            self._ensure_view_window_locked()
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
                "spinner-text-highlight": "#ffffff bold",
                # Completion menu styling
                "completion-menu": "bg:#1a1a2e #cccccc",
                "completion-menu.completion": "bg:#1a1a2e #cccccc",
                "completion-menu.completion.current": "bg:#444444 #ffffff bold",
                "completion-menu.meta.completion": "bg:#1a1a2e #888888 italic",
                "completion-menu.meta.completion.current": "bg:#444444 #aaaaaa italic",
                "copy-button": "bg:#444444 #ffffff bold",
                "inline-spinner": "#ffaa00 bold",
                "fold-toggle": "#cccccc bold",
            })
        else:
            self._style = Style.from_dict({})

    def append_output(self, text: str) -> None:
        """Append text to the output area (thread-safe)."""
        with self._output_lock:
            text = "" if text is None else str(text)
            new_lines = text.split("\n")
            if self._output_lines == [""]:
                self._output_lines = new_lines
            else:
                self._output_lines.extend(new_lines)
            if not self._output_lines:
                self._output_lines = [""]
            self._output_line_count = max(1, len(self._output_lines))
            self._output_version += 1

            # Auto-scroll to bottom only when following output.
            if self._follow_output:
                self._scroll_offset = max(0, self._output_line_count - 1)
                self._scroll_col = 10**9
            else:
                # Keep current view, but make sure it's still a valid offset.
                self._scroll_offset = max(0, min(self._scroll_offset, self._output_line_count - 1))
                self._scroll_col = max(0, int(self._scroll_col or 0))
            self._ensure_view_window_locked()

        # Trigger UI refresh (now safe - cache updated atomically)
        if self._app and self._app.is_running:
            self._app.invalidate()

    def clear_output(self) -> None:
        """Clear the output area (thread-safe)."""
        with self._output_lock:
            self._output_lines = [""]
            self._output_line_count = 1
            self._output_version += 1
            self._scroll_offset = 0
            self._scroll_col = 0
            self._follow_output = True
            self._view_start = 0
            self._view_end = 1
            self._copy_payloads.clear()

        if self._app and self._app.is_running:
            self._app.invalidate()

    def set_output(self, text: str) -> None:
        """Replace all output with new text (thread-safe)."""
        with self._output_lock:
            text = "" if text is None else str(text)
            self._output_lines = text.split("\n") if text else [""]
            self._output_line_count = max(1, len(self._output_lines))
            self._output_version += 1
            self._scroll_offset = 0
            self._scroll_col = 0
            self._follow_output = True
            self._view_start = 0
            self._view_end = min(self._output_line_count, len(self._output_lines)) or 1
            self._copy_payloads.clear()

        if self._app and self._app.is_running:
            self._app.invalidate()

    def _advance_spinner_frame(self) -> None:
        """Advance spinner animation counters by one tick.

        `_spinner_frame` is intentionally monotonic: the status-bar shimmer uses it
        to select which character(s) to highlight. If `_spinner_frame` were wrapped
        by the number of spinner glyph frames (typically 10), the shimmer would
        never reach beyond the first ~10 visible characters of long status texts.
        """
        self._spinner_frame += 1

    def _spinner_loop(self) -> None:
        """Background thread that animates the spinner."""
        while self._spinner_active and not self._shutdown:
            self._advance_spinner_frame()
            if self._app and self._app.is_running:
                self._app.invalidate()
            time.sleep(0.1)  # 10 FPS animation

    def set_spinner(self, text: str, *, duration_s: Optional[float] = None) -> None:
        """Start the spinner with the given text (thread-safe).

        Args:
            text: Status text to show next to the spinner (e.g., "Generating...")
            duration_s: Optional auto-clear timeout in seconds.
                - If None or <= 0: spinner stays until explicitly cleared or replaced
                - If > 0: spinner auto-clears after the timeout unless superseded by a newer spinner text
        """
        # Invalidate any previous auto-clear timer.
        self._spinner_token += 1
        token = self._spinner_token
        if self._spinner_clear_timer:
            try:
                self._spinner_clear_timer.cancel()
            except Exception:
                pass
            self._spinner_clear_timer = None

        self._spinner_text = text
        self._spinner_frame = 0

        if not self._spinner_active:
            self._spinner_active = True
            self._spinner_thread = threading.Thread(target=self._spinner_loop, daemon=True)
            self._spinner_thread.start()
        elif self._app and self._app.is_running:
            self._app.invalidate()

        # Schedule optional auto-clear.
        try:
            dur = float(duration_s) if duration_s is not None else None
        except Exception:
            dur = None
        if dur is not None and dur > 0:
            def _clear_if_current() -> None:
                if self._spinner_token != token:
                    return
                self.clear_spinner()

            t = threading.Timer(dur, _clear_if_current)
            t.daemon = True
            self._spinner_clear_timer = t
            t.start()

    def clear_spinner(self) -> None:
        """Stop and hide the spinner (thread-safe)."""
        # Cancel any pending auto-clear (if any).
        self._spinner_token += 1
        if self._spinner_clear_timer:
            try:
                self._spinner_clear_timer.cancel()
            except Exception:
                pass
            self._spinner_clear_timer = None

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
