//! Theme-contrast audit for the chrome's ink choices (cycle-2 review).
//!
//! The chrome renders INFORMATION in `text_muted` (header facts, footer
//! instruments, session id) and grades the ctx meter in `warn`/`error` —
//! all against `surface` (header/footer rows fill with it). This audit
//! measures WCAG contrast for those exact pairs across every built-in
//! theme, so a theme registry drift that would make an instrument
//! unreadable fails HERE instead of on an operator's terminal.
//!
//! Floors asserted (UI-component floor, not prose-text AA). Measured
//! minima across the shipped 26 themes (2026-07-23 audit): muted 4.54
//! (everforest-light), warn 3.75 (catppuccin-latte), error 3.77
//! (solarized-light), text 5.40 (everforest-light) — the asserts sit
//! below the measured minima but catch a registry drift that would ship
//! an unreadable instrument:
//! - `text_muted` on `surface` ≥ 3.0:1 — information-carrying ink (the
//!   2026-07-22 review moved the session id OFF `text_faint` for exactly
//!   this floor; the muted tier must actually clear it everywhere).
//! - `warn`/`error` on `surface` ≥ 3.0:1 — the graded ctx meter tones.
//! - `text` on `surface` ≥ 4.5:1 — card values on the idle card.
//!
//! `text_faint` is deliberately NOT floored: the theme contract calls it
//! decoration-only, and chrome uses it only for separators/hints — the
//! same audit measured faint bottoming at 2.77 (abstract-dark), which is
//! exactly why info-carrying chrome must never use it.

fn luminance(c: abstracttui::prelude::Rgba) -> f64 {
    let chan = |v: u8| {
        let s = v as f64 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * chan(c.r) + 0.7152 * chan(c.g) + 0.0722 * chan(c.b)
}

/// WCAG contrast ratio between two OPAQUE colors (chrome fills the
/// surface first, so the effective pair is exactly (ink, surface)).
fn contrast(fg: abstracttui::prelude::Rgba, bg: abstracttui::prelude::Rgba) -> f64 {
    let (a, b) = (luminance(fg), luminance(bg));
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

/// The splash animation's SEPARATION floors (refinement-pass P2): the
/// shimmer lerps `text_muted → highlight` and the mark breath lerps
/// `text_faint → highlight` — if the endpoints are near-identical the
/// animation is invisible (measured pre-fix: catppuccin-mocha muted→
/// accent at 1.15:1, nord faint→accent at 1.08:1 — five themes with a
/// dead animation). `logo::floored_highlights` walks the accent toward
/// `text` until each pair clears its floor (the engine's
/// `mix_until_contrast`, the registry's own syntax-ink pattern); this
/// audit pins that the DERIVED pairs clear those floors on every theme,
/// so a registry drift or a floor typo fails here, not on an operator's
/// terminal.
#[test]
fn splash_animation_endpoints_separate_on_every_theme() {
    let mut failures: Vec<String> = Vec::new();
    for theme in abstracttui::theme::themes() {
        let t = theme.tokens;
        let (shimmer_hi, mark_hi) = abstractcode::ui::logo::floored_highlights(&t);
        // Audit floors sit just UNDER the weakest theme's achievable
        // ceiling (measured 2026-07-23: monokai's muted↔pole tops out
        // at 1.30 — its muted ink is already near-white, so no
        // theme-derived highlight can separate further). The walk in
        // `floored_highlights` targets 1.35/1.6 and saturates at the
        // pole; this audit catches REGRESSIONS (a registry drift or a
        // recipe change that dims the animation), not the impossible.
        let checks: [(&str, f64, f64); 2] = [
            (
                "shimmer muted→highlight",
                contrast(t.text_muted, shimmer_hi),
                1.28,
            ),
            ("mark faint→highlight", contrast(t.text_faint, mark_hi), 1.5),
        ];
        for (name, ratio, floor) in checks {
            if ratio < floor {
                failures.push(format!(
                    "{}: {name} = {ratio:.2}:1 (floor {floor}:1)",
                    theme.id
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "splash animation endpoints below their separation floor:\n{}",
        failures.join("\n")
    );
}

#[test]
fn chrome_ink_pairs_clear_their_floors_on_every_theme() {
    let mut failures: Vec<String> = Vec::new();
    for theme in abstracttui::theme::themes() {
        let t = theme.tokens;
        let checks: [(&str, f64, f64); 4] = [
            ("text_muted/surface", contrast(t.text_muted, t.surface), 3.0),
            ("warn/surface", contrast(t.warn, t.surface), 3.0),
            ("error/surface", contrast(t.error, t.surface), 3.0),
            ("text/surface", contrast(t.text, t.surface), 4.5),
        ];
        for (name, ratio, floor) in checks {
            if ratio < floor {
                failures.push(format!(
                    "{}: {name} = {ratio:.2}:1 (floor {floor}:1)",
                    theme.id
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "chrome ink pairs below their contrast floor:\n{}",
        failures.join("\n")
    );
}
