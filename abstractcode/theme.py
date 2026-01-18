from __future__ import annotations

from dataclasses import dataclass
import os
import re
from typing import Dict, Optional, Tuple


_HEX_RE = re.compile(r"^#?[0-9a-fA-F]{6}$")


def _clamp_u8(x: int) -> int:
    return 0 if x < 0 else 255 if x > 255 else int(x)


def _hex_to_rgb(hex_color: str) -> Tuple[int, int, int]:
    s = str(hex_color or "").strip()
    if not s:
        return (0, 0, 0)
    if not s.startswith("#"):
        s = "#" + s
    if not _HEX_RE.match(s):
        return (0, 0, 0)
    r = int(s[1:3], 16)
    g = int(s[3:5], 16)
    b = int(s[5:7], 16)
    return (r, g, b)


def _rgb_to_hex(rgb: Tuple[int, int, int]) -> str:
    r, g, b = rgb
    return f"#{_clamp_u8(r):02x}{_clamp_u8(g):02x}{_clamp_u8(b):02x}"


def ansi_fg(hex_color: str) -> str:
    """Return a truecolor ANSI foreground escape for a hex color (or "" if invalid)."""
    s = str(hex_color or "").strip()
    if not s:
        return ""
    if not s.startswith("#"):
        s = "#" + s
    if not _HEX_RE.match(s):
        return ""
    r, g, b = _hex_to_rgb(s)
    return f"\033[38;2;{r};{g};{b}m"


def ansi_bg(hex_color: str) -> str:
    """Return a truecolor ANSI background escape for a hex color (or "" if invalid)."""
    s = str(hex_color or "").strip()
    if not s:
        return ""
    if not s.startswith("#"):
        s = "#" + s
    if not _HEX_RE.match(s):
        return ""
    r, g, b = _hex_to_rgb(s)
    return f"\033[48;2;{r};{g};{b}m"


def blend_hex(a: str, b: str, t: float) -> str:
    """Blend two hex colors (t=0 -> a, t=1 -> b)."""
    t = 0.0 if t < 0 else 1.0 if t > 1 else float(t)
    ar, ag, ab = _hex_to_rgb(a)
    br, bg, bb = _hex_to_rgb(b)
    r = int(ar + (br - ar) * t)
    g = int(ag + (bg - ag) * t)
    b2 = int(ab + (bb - ab) * t)
    return _rgb_to_hex((r, g, b2))


def relative_luminance(hex_color: str) -> float:
    """Return relative luminance (0..1) for an sRGB hex color."""
    r, g, b = _hex_to_rgb(hex_color)

    def _to_linear(u8: int) -> float:
        x = max(0.0, min(1.0, float(u8) / 255.0))
        return x / 12.92 if x <= 0.04045 else ((x + 0.055) / 1.055) ** 2.4

    rl = _to_linear(r)
    gl = _to_linear(g)
    bl = _to_linear(b)
    return 0.2126 * rl + 0.7152 * gl + 0.0722 * bl


def is_dark(hex_color: str, *, threshold: float = 0.40) -> bool:
    """Heuristic for whether a color should be treated as a dark surface."""
    try:
        return relative_luminance(hex_color) < float(threshold)
    except Exception:
        return True


def normalize_hex(hex_color: str, *, fallback: str) -> str:
    s = str(hex_color or "").strip()
    if not s:
        return fallback
    if not s.startswith("#"):
        s = "#" + s
    if not _HEX_RE.match(s):
        return fallback
    return s.lower()


@dataclass(frozen=True)
class Theme:
    """A small set of design tokens for the TUI.

    We keep this intentionally small (4 base colors) and derive everything else.
    """

    name: str
    primary: str
    secondary: str
    surface: str
    muted: str

    def normalized(self) -> "Theme":
        return Theme(
            name=str(self.name or "theme").strip() or "theme",
            primary=normalize_hex(self.primary, fallback="#00aa00"),
            secondary=normalize_hex(self.secondary, fallback="#00aaff"),
            surface=normalize_hex(self.surface, fallback="#1a1a2e"),
            muted=normalize_hex(self.muted, fallback="#888888"),
        )


BUILTIN_THEMES: Dict[str, Theme] = {
    # Close to the existing palette; good default for dark terminals.
    "midnight": Theme(
        name="midnight",
        primary="#00aa00",
        secondary="#00aaff",
        surface="#1a1a2e",
        muted="#888888",
    ),
    # Tokyo Night base, but with an orange secondary accent for footer/help.
    "tokyo": Theme(
        name="tokyo",
        primary="#7aa2f7",
        secondary="#ff9e64",
        surface="#1a1b26",
        muted="#565f89",
    ),
    "tokyo-night": Theme(
        name="tokyo-night",
        primary="#7aa2f7",
        secondary="#bb9af7",
        surface="#1a1b26",
        muted="#565f89",
    ),
    "dracula": Theme(
        name="dracula",
        primary="#50fa7b",
        secondary="#bd93f9",
        surface="#282a36",
        muted="#6272a4",
    ),
    "nord": Theme(
        name="nord",
        primary="#88c0d0",
        secondary="#81a1c1",
        surface="#2e3440",
        muted="#8fbcbb",
    ),
    "gruvbox-dark": Theme(
        name="gruvbox-dark",
        primary="#b8bb26",
        secondary="#fabd2f",
        surface="#282828",
        muted="#a89984",
    ),
    # Additional one-word themes (more visually distinct).
    "aurora": Theme(
        name="aurora",
        primary="#22c55e",
        secondary="#a78bfa",
        surface="#0b1021",
        muted="#64748b",
    ),
    "ember": Theme(
        name="ember",
        primary="#f97316",
        secondary="#fb7185",
        surface="#160b10",
        muted="#9ca3af",
    ),
    "ocean": Theme(
        name="ocean",
        primary="#38bdf8",
        secondary="#34d399",
        surface="#071a2b",
        muted="#64748b",
    ),
    "coral": Theme(
        name="coral",
        primary="#fb923c",
        secondary="#34d399",
        surface="#071a2b",
        muted="#64748b",
    ),
    "paper": Theme(
        name="paper",
        primary="#2563eb",
        secondary="#7c3aed",
        surface="#f8fafc",
        muted="#334155",
    ),
}


def _env(name: str) -> str:
    return str(os.getenv(name, "") or "").strip()


def theme_from_env(*, default: str = "tokyo") -> Theme:
    name = _env("ABSTRACTCODE_THEME") or default
    base = BUILTIN_THEMES.get(name.lower(), BUILTIN_THEMES.get(default, next(iter(BUILTIN_THEMES.values()))))
    t = base.normalized()

    # Optional overrides (lets users define up to 4 base colors without code changes).
    primary = _env("ABSTRACTCODE_THEME_PRIMARY")
    secondary = _env("ABSTRACTCODE_THEME_SECONDARY")
    surface = _env("ABSTRACTCODE_THEME_SURFACE")
    muted = _env("ABSTRACTCODE_THEME_MUTED")
    if any([primary, secondary, surface, muted]):
        t = Theme(
            name="custom",
            primary=normalize_hex(primary, fallback=t.primary),
            secondary=normalize_hex(secondary, fallback=t.secondary),
            surface=normalize_hex(surface, fallback=t.surface),
            muted=normalize_hex(muted, fallback=t.muted),
        ).normalized()

    return t


def get_theme(name: str, *, default: str = "tokyo") -> Optional[Theme]:
    n = str(name or "").strip().lower()
    if not n:
        return None
    if n == "custom":
        return theme_from_env(default=default)
    return BUILTIN_THEMES.get(n)
