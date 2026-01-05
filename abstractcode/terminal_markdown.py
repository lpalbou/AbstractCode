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

    def render(self, text: str) -> str:
        s = "" if text is None else str(text)
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



