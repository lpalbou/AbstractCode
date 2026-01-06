"""Minimal, terminal-friendly Markdown renderer.

Goal: improve readability in the TUI without attempting full CommonMark compliance.
We deliberately keep this conservative:
- Only style headings, code fences, and a few inline constructs.
- Never mutate the underlying content used for copy-to-clipboard.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import List


@dataclass(frozen=True)
class AnsiPalette:
    reset: str = "\033[0m"
    dim: str = "\033[2m"
    bold: str = "\033[1m"
    cyan: str = "\033[36m"
    green: str = "\033[32m"
    blue: str = "\033[38;5;39m"


class TerminalMarkdownRenderer:
    """Render a subset of Markdown to ANSI-styled plain text."""

    _re_heading = re.compile(r"^(?P<hashes>#{1,6})\s+(?P<title>.+?)\s*$")
    _re_hr = re.compile(r"^\s*(-{3,}|_{3,}|\*{3,})\s*$")
    _re_bold = re.compile(r"\*\*(?P<txt>[^*]+)\*\*")
    _re_inline_code = re.compile(r"`(?P<code>[^`]+)`")

    def __init__(self, *, color: bool = True, palette: AnsiPalette | None = None) -> None:
        self._color = bool(color)
        self._p = palette or AnsiPalette()

    def _style(self, text: str, *codes: str) -> str:
        if not self._color or not codes:
            return text
        return "".join(codes) + text + self._p.reset

    def _style_inline(self, line: str) -> str:
        # Bold
        def _bold(m: re.Match) -> str:
            return self._style(m.group("txt"), self._p.bold)

        # Inline code
        def _code(m: re.Match) -> str:
            return self._style(m.group("code"), self._p.blue)

        out = self._re_bold.sub(_bold, line)
        out = self._re_inline_code.sub(_code, out)
        return out

    def _unescape_newlines_if_needed(self, s: str) -> str:
        """Convert literal "\\n" / "\\r" / "\\r\\n" sequences into real newlines.

        Some upstream layers accidentally pass serialized strings (repr/json) where newlines are
        encoded as the two characters backslash+n. We only unescape when the input has *no* real
        newlines to avoid corrupting valid code like `print("a\\nb")`.
        """
        if "\n" in s or "\r" in s:
            return s
        if "\\n" not in s and "\\r" not in s:
            return s

        out: List[str] = []
        i = 0
        n = len(s)
        while i < n:
            ch = s[i]
            if ch != "\\":
                out.append(ch)
                i += 1
                continue

            # Count consecutive backslashes.
            j = i
            while j < n and s[j] == "\\":
                j += 1
            run_len = j - i

            if j >= n:
                out.append("\\" * run_len)
                break

            nxt = s[j]

            # Only treat "\n"/"\r" as escapes when the escape backslash is not itself escaped.
            if nxt in ("n", "r") and (run_len % 2 == 1):
                # Preserve all but the escape backslash.
                if run_len > 1:
                    out.append("\\" * (run_len - 1))
                out.append("\n")
                i = j + 1

                # Collapse \r\n into a single newline (Windows-style payloads).
                if nxt == "r" and i < n and s[i] == "\\":
                    k = i
                    while k < n and s[k] == "\\":
                        k += 1
                    run2_len = k - i
                    if k < n and s[k] == "n" and (run2_len % 2 == 1):
                        if run2_len > 1:
                            out.append("\\" * (run2_len - 1))
                        i = k + 1
                continue

            # Not an escape we handle; emit literally.
            out.append("\\" * run_len)
            out.append(nxt)
            i = j + 1

        return "".join(out)

    def render(self, text: str) -> str:
        s = "" if text is None else str(text)
        s = self._unescape_newlines_if_needed(s)
        lines = s.splitlines()
        out: List[str] = []

        in_code = False
        fence_lang = ""

        for raw in lines:
            line = raw.rstrip("\n")

            # Code fences
            if line.strip().startswith("```"):
                if not in_code:
                    in_code = True
                    fence_lang = line.strip()[3:].strip()
                    label = f"code" + (f" ({fence_lang})" if fence_lang else "")
                    out.append(self._style(f"┌─ {label}", self._p.dim))
                else:
                    in_code = False
                    out.append(self._style("└─", self._p.dim))
                continue

            if in_code:
                # Keep code unmodified; add a subtle gutter.
                out.append(self._style("│ ", self._p.dim) + line)
                continue

            # Horizontal rules
            if self._re_hr.match(line):
                out.append(self._style("─" * 60, self._p.dim))
                continue

            # Headings
            m = self._re_heading.match(line)
            if m:
                hashes = m.group("hashes")
                title = m.group("title").strip()
                level = len(hashes)
                if level <= 2:
                    out.append(self._style(title, self._p.bold, self._p.cyan))
                else:
                    out.append(self._style(title, self._p.bold))
                continue

            out.append(self._style_inline(line))

        return "\n".join(out)



