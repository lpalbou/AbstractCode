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

from .theme import Theme, theme_from_env


@dataclass(frozen=True)
class AnsiPalette:
    reset: str = "\033[0m"
    dim: str = "\033[2m"
    bold: str = "\033[1m"
    italic: str = "\033[3m"
    underline: str = "\033[4m"
    cyan: str = "\033[36m"
    green: str = "\033[32m"
    blue: str = "\033[38;5;39m"


def _ansi_fg(hex_color: str) -> str:
    s = str(hex_color or "").strip()
    if not s:
        return ""
    if not s.startswith("#"):
        s = "#" + s
    if len(s) != 7:
        return ""
    try:
        r = int(s[1:3], 16)
        g = int(s[3:5], 16)
        b = int(s[5:7], 16)
    except Exception:
        return ""
    return f"\033[38;2;{r};{g};{b}m"


def _palette_from_theme(theme: Theme) -> AnsiPalette:
    t = theme.normalized()
    primary = _ansi_fg(t.primary) or AnsiPalette.cyan
    secondary = _ansi_fg(t.secondary) or AnsiPalette.blue
    return AnsiPalette(cyan=primary, green=primary, blue=secondary)


class TerminalMarkdownRenderer:
    """Render a subset of Markdown to ANSI-styled plain text."""

    _re_heading = re.compile(r"^(?P<hashes>#{1,6})\s+(?P<title>.+?)\s*$")
    _re_hr = re.compile(r"^\s*(-{3,}|_{3,}|\*{3,})\s*$")
    _re_inline_code = re.compile(r"`(?P<code>[^`]+)`")
    _re_md_link = re.compile(r"\[(?P<label>[^\]]+)\]\((?P<url>[^)\s]+)(?:\s+\"[^\"]*\")?\)")
    _re_autolink = re.compile(r"<(?P<url>https?://[^>]+)>")
    _re_bare_url = re.compile(r"(?P<url>https?://[^\s<>()\]]+)")
    _re_bold = re.compile(r"(\*\*|__)(?P<txt>.+?)\1")
    _re_strike = re.compile(r"~~(?P<txt>[^~]+)~~")
    _re_italic_star = re.compile(r"(?<!\*)\*(?P<txt>[^*]+?)\*(?!\*)")
    _re_italic_us = re.compile(r"(?<!_)_(?P<txt>[^_]+?)_(?!_)")
    _re_blockquote = re.compile(r"^\s*>\s?(?P<body>.*)$")
    _re_task = re.compile(r"^(?P<indent>\s*)[-*+]\s+\[(?P<state>[ xX])\]\s+(?P<body>.*)$")
    _re_bullet = re.compile(r"^(?P<indent>\s*)[-*+]\s+(?P<body>.*)$")
    _re_ordered = re.compile(r"^(?P<indent>\s*)(?P<num>\d+)[.)]\s+(?P<body>.*)$")
    _re_table_sep_cell = re.compile(r"^:?-{3,}:?$")
    _re_ansi = re.compile(r"\x1b\[[0-9;]*m")

    def __init__(
        self,
        *,
        color: bool = True,
        theme: Theme | None = None,
        palette: AnsiPalette | None = None,
        width: int | None = None,
    ) -> None:
        self._color = bool(color)
        if palette is not None:
            self._p = palette
        else:
            self._p = _palette_from_theme(theme or theme_from_env()) if self._color else AnsiPalette()
        try:
            w = int(width) if width is not None else None
        except Exception:
            w = None
        self._width = max(40, w) if isinstance(w, int) else 120

    def _style(self, text: str, *codes: str) -> str:
        if not self._color or not codes:
            return text
        return "".join(codes) + text + self._p.reset

    def _strip_ansi(self, s: str) -> str:
        return self._re_ansi.sub("", str(s or ""))

    def _split_inline_code(self, s: str) -> List[tuple[str, str]]:
        """Split into [('text'|'code', chunk), ...] preserving order."""
        out: List[tuple[str, str]] = []
        pos = 0
        for m in self._re_inline_code.finditer(s):
            if m.start() > pos:
                out.append(("text", s[pos : m.start()]))
            out.append(("code", m.group("code")))
            pos = m.end()
        if pos < len(s):
            out.append(("text", s[pos:]))
        if not out:
            out.append(("text", s))
        return out

    def _split_url_trailing_punct(self, url: str) -> tuple[str, str]:
        u = str(url or "")
        trailing = ""
        while u and u[-1] in ".,;:)]}":
            trailing = u[-1] + trailing
            u = u[:-1]
        return u, trailing

    def _style_url(self, url: str) -> str:
        clean, trailing = self._split_url_trailing_punct(url)
        if not clean:
            return url
        return self._style(clean, self._p.blue, self._p.underline) + trailing

    def _render_inline_text(self, s: str) -> str:
        """Render inline Markdown for non-code text segments."""

        def _md_link(m: re.Match) -> str:
            label = str(m.group("label") or "")
            url = str(m.group("url") or "")
            if not url:
                return label
            label_s = self._style(label, self._p.blue, self._p.underline) if label else self._style_url(url)
            return f"{label_s} {self._style('↗', self._p.dim)} {self._style_url(url)}"

        def _auto(m: re.Match) -> str:
            return self._style_url(str(m.group("url") or ""))

        # Links first (to avoid emphasis rules eating brackets/parentheses).
        out = self._re_md_link.sub(_md_link, s)
        out = self._re_autolink.sub(_auto, out)

        # Bare URLs.
        def _bare(m: re.Match) -> str:
            return self._style_url(str(m.group("url") or ""))

        out = self._re_bare_url.sub(_bare, out)

        # Bold / strike / italic.
        out = self._re_bold.sub(lambda m: self._style(m.group("txt"), self._p.bold), out)
        out = self._re_strike.sub(lambda m: self._style(m.group("txt"), self._p.dim), out)
        out = self._re_italic_star.sub(lambda m: self._style(m.group("txt"), self._p.italic), out)
        out = self._re_italic_us.sub(lambda m: self._style(m.group("txt"), self._p.italic), out)
        return out

    def _render_inline(self, line: str) -> str:
        """Render a line with inline Markdown (code + emphasis + links)."""
        out: List[str] = []
        for kind, chunk in self._split_inline_code(str(line or "")):
            if not chunk:
                continue
            if kind == "code":
                out.append(self._style(chunk, self._p.blue))
            else:
                out.append(self._render_inline_text(chunk))
        return "".join(out)

    def _is_table_separator(self, line: str) -> bool:
        s = str(line or "").strip()
        if not s or "|" not in s:
            return False
        s = s.strip("|").replace(" ", "")
        if not s:
            return False
        parts = s.split("|")
        if not parts or any(not p for p in parts):
            return False
        return all(self._re_table_sep_cell.match(p or "") for p in parts)

    def _split_table_row(self, line: str) -> List[str]:
        s = str(line or "").strip()
        if s.startswith("|"):
            s = s[1:]
        if s.endswith("|"):
            s = s[:-1]
        parts = [p.strip() for p in s.split("|")]
        return parts

    def _table_alignments(self, sep_line: str, ncols: int) -> List[str]:
        parts = self._split_table_row(sep_line)
        aligns: List[str] = []
        for p in parts[:ncols]:
            cell = str(p or "").strip()
            left = cell.startswith(":")
            right = cell.endswith(":")
            if left and right:
                aligns.append("center")
            elif right:
                aligns.append("right")
            else:
                aligns.append("left")
        while len(aligns) < ncols:
            aligns.append("left")
        return aligns

    def _truncate_plain(self, s: str, max_len: int) -> str:
        txt = str(s or "").replace("\t", "    ").replace("\r", "")
        txt = txt.replace("\n", " ")
        if max_len <= 0:
            return ""
        if len(txt) <= max_len:
            return txt
        if max_len == 1:
            return "…"
        return txt[: max_len - 1] + "…"

    def _render_table(self, header: List[str], sep_line: str, rows: List[List[str]]) -> List[str]:
        ncols = max(1, len(header))
        aligns = self._table_alignments(sep_line, ncols=ncols)

        norm_rows: List[List[str]] = []
        for r in rows:
            rr = list(r or [])
            if len(rr) < ncols:
                rr.extend([""] * (ncols - len(rr)))
            norm_rows.append(rr[:ncols])

        # Compute widths from plain (un-styled) content.
        widths = [0] * ncols
        for c in range(ncols):
            widths[c] = max(widths[c], len(self._strip_ansi(str(header[c] if c < len(header) else ""))))
        for r in norm_rows:
            for c in range(ncols):
                widths[c] = max(widths[c], len(self._strip_ansi(str(r[c] or ""))))

        # Fit into available width (best-effort; avoid wrapping table borders).
        max_total = max(40, int(self._width or 120))
        # Borders: 1 + (ncols-1) + 1 = ncols+1, plus padding " " around cells (2*ncols).
        border_cost = (ncols + 1) + (2 * ncols)
        avail_cells = max(1, max_total - border_cost)
        total_cells = sum(widths)
        if total_cells > avail_cells:
            # Shrink proportionally, but keep a minimum for readability.
            min_w = 6
            widths = [max(min_w, w) for w in widths]
            total_cells = sum(widths)
            if total_cells > avail_cells:
                # Still too wide: hard-cap each column.
                cap = max(min_w, avail_cells // ncols)
                widths = [min(w, cap) for w in widths]

        def hline(left: str, mid: str, right: str) -> str:
            segs = ["─" * (w + 2) for w in widths]
            return self._style(left + mid.join(segs) + right, self._p.dim)

        def render_row(cells: List[str], *, header_row: bool) -> str:
            parts: List[str] = []
            for i, w in enumerate(widths):
                raw = str(cells[i] if i < len(cells) else "")
                plain = self._truncate_plain(raw, w)
                if aligns[i] == "right":
                    plain = plain.rjust(w)
                elif aligns[i] == "center":
                    plain = plain.center(w)
                else:
                    plain = plain.ljust(w)
                styled = self._render_inline(plain)
                if header_row:
                    styled = self._style(styled, self._p.bold, self._p.cyan)
                parts.append(f" {styled} ")
            inner = "│".join(parts)
            return "│" + inner + "│"

        out: List[str] = []
        out.append(hline("┌", "┬", "┐"))
        out.append(render_row(header, header_row=True))
        out.append(hline("├", "┼", "┤"))
        for r in norm_rows:
            out.append(render_row(r, header_row=False))
        out.append(hline("└", "┴", "┘"))
        return out

    def _render_mermaid(self, code_lines: List[str]) -> List[str]:
        """Best-effort text rendering for common Mermaid diagrams."""
        lines = [str(l or "").rstrip() for l in (code_lines or [])]
        non_empty = [ln for ln in lines if ln.strip()]
        if not non_empty:
            return []

        head = non_empty[0].strip()
        items: List[str] = []

        def add(line: str) -> None:
            if line:
                items.append(line)

        if head.startswith(("graph", "flowchart")):
            arrows = ["-->", "==>", "-.->", "---", "--", "->>"]
            for ln in non_empty[1:]:
                raw = ln.strip()
                if not raw or raw.startswith("%%"):
                    continue
                # Strip inline labels like -->|label|
                raw = re.sub(r"\|[^|]*\|", "", raw)
                found = None
                for a in arrows:
                    if a in raw:
                        found = a
                        break
                if not found:
                    continue
                left, right = raw.split(found, 1)
                left = left.strip()
                right = right.strip()
                for sep in ("[", "(", "{", "<"):
                    if sep in left:
                        left = left.split(sep, 1)[0].strip()
                    if sep in right:
                        right = right.split(sep, 1)[0].strip()
                if not left or not right:
                    continue
                add(f"• {left} → {right}")
        elif head.startswith("sequenceDiagram"):
            msg_re = re.compile(r"^(?P<a>[^-]+?)-+>>?(?P<b>[^:]+?):\\s*(?P<msg>.+)$")
            for ln in non_empty[1:]:
                raw = ln.strip()
                if not raw or raw.startswith("%%"):
                    continue
                m = msg_re.match(raw)
                if not m:
                    continue
                a = m.group("a").strip()
                b = m.group("b").strip()
                msg = m.group("msg").strip()
                add(f"• {a} ⇢ {b}: {msg}")
        else:
            # Unknown kind: provide a lightweight summary.
            add(f"• (unrecognized mermaid; showing source)")

        if not items:
            add("• (no edges/messages parsed; showing source)")

        # Always show the source too (still inside the fenced block).
        out: List[str] = []
        for it in items[:40]:
            out.append(self._style("│ ", self._p.dim) + self._render_inline(it))
        if len(items) > 40:
            out.append(self._style("│ … (more)", self._p.dim))
        out.append(self._style("│", self._p.dim))
        out.append(self._style("│ source:", self._p.dim))
        for ln in lines[:60]:
            out.append(self._style("│ ", self._p.dim) + ln)
        if len(lines) > 60:
            out.append(self._style("│ … (source truncated)", self._p.dim))
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

        i = 0
        while i < len(lines):
            raw = lines[i]
            line = str(raw or "").rstrip("\n")

            # Code fences (block-level).
            stripped = line.strip()
            if stripped.startswith("```"):
                fence_lang = stripped[3:].strip().lower()
                code_lines: List[str] = []
                i += 1
                while i < len(lines):
                    candidate = str(lines[i] or "").rstrip("\n")
                    if candidate.strip().startswith("```"):
                        break
                    code_lines.append(candidate)
                    i += 1

                label = "code" + (f" ({fence_lang})" if fence_lang else "")
                if fence_lang == "mermaid":
                    label = "diagram (mermaid)"
                out.append(self._style(f"┌─ {label}", self._p.dim))
                if fence_lang == "mermaid":
                    rendered = self._render_mermaid(code_lines)
                    if rendered:
                        out.extend(rendered)
                    else:
                        for ln in code_lines:
                            out.append(self._style("│ ", self._p.dim) + ln)
                else:
                    for ln in code_lines:
                        out.append(self._style("│ ", self._p.dim) + ln)
                out.append(self._style("└─", self._p.dim))
                # Skip the closing fence if present.
                if i < len(lines) and str(lines[i] or "").strip().startswith("```"):
                    i += 1
                continue

            # Tables (header + separator line).
            if i + 1 < len(lines) and "|" in line and self._is_table_separator(lines[i + 1]):
                header = self._split_table_row(line)
                sep = str(lines[i + 1] or "")
                body: List[List[str]] = []
                i += 2
                while i < len(lines):
                    row_line = str(lines[i] or "").rstrip("\n")
                    if not row_line.strip() or "|" not in row_line:
                        break
                    body.append(self._split_table_row(row_line))
                    i += 1
                out.extend(self._render_table(header, sep, body))
                continue

            # Horizontal rules
            if self._re_hr.match(line):
                out.append(self._style("─" * 60, self._p.dim))
                i += 1
                continue

            # Headings
            m = self._re_heading.match(line)
            if m:
                title = m.group("title").strip()
                level = len(m.group("hashes"))
                rendered_title = self._render_inline(title)
                plain_title = self._strip_ansi(rendered_title)
                if level <= 2:
                    if self._color:
                        base = self._p.bold + self._p.cyan
                        # Keep heading styling applied after inline segments reset.
                        rendered_title = rendered_title.replace(self._p.reset, self._p.reset + base)
                        out.append(base + rendered_title + self._p.reset)
                    else:
                        out.append(rendered_title)
                    underline = "─" * max(8, min(60, len(plain_title)))
                    out.append(self._style(underline, self._p.dim))
                else:
                    if self._color:
                        base = self._p.bold
                        rendered_title = rendered_title.replace(self._p.reset, self._p.reset + base)
                        out.append(base + rendered_title + self._p.reset)
                    else:
                        out.append(rendered_title)
                i += 1
                continue

            # Blockquotes
            m = self._re_blockquote.match(line)
            if m:
                body = str(m.group("body") or "")
                out.append(self._style("│ ", self._p.dim) + self._render_inline(body))
                i += 1
                continue

            # Task lists
            m = self._re_task.match(line)
            if m:
                indent = str(m.group("indent") or "")
                state = str(m.group("state") or " ").lower()
                body = str(m.group("body") or "")
                box = "☑" if state == "x" else "☐"
                prefix = self._style(box, self._p.cyan, self._p.bold)
                out.append(f"{indent}{prefix} {self._render_inline(body)}")
                i += 1
                continue

            # Bullets / ordered lists
            m = self._re_bullet.match(line)
            if m:
                indent = str(m.group("indent") or "")
                body = str(m.group("body") or "")
                bullet = self._style("•", self._p.cyan, self._p.bold)
                out.append(f"{indent}{bullet} {self._render_inline(body)}")
                i += 1
                continue
            m = self._re_ordered.match(line)
            if m:
                indent = str(m.group("indent") or "")
                num = str(m.group("num") or "").strip()
                body = str(m.group("body") or "")
                num_s = self._style(f"{num}.", self._p.cyan, self._p.bold)
                out.append(f"{indent}{num_s} {self._render_inline(body)}")
                i += 1
                continue

            out.append(self._render_inline(line))
            i += 1

        return "\n".join(out)
