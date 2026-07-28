//! The AbstractCode splash logo (IDLE-2): a compact two-row half-block
//! wordmark under a two-row `▲` mark, with a slow theme-ink shimmer
//! sweeping the letters and a soft breath on the mark.
//!
//! Design rules:
//! - THEME INKS ONLY, with a SEPARATION FLOOR: the shimmer/pulse
//!   highlights are the theme's accent walked toward `text` until they
//!   clear a contrast floor against their base ink (the engine's
//!   `mix_until_contrast`, the registry's own syntax-ink pattern) — on
//!   themes where muted ≈ accent (catppuccin family, everforest, nord's
//!   faint) the animation would otherwise be invisible; healthy themes
//!   get their accent unchanged. Pinned across all themes in
//!   tests/theme_contrast_audit.rs.
//! - HONEST DEGRADATION both axes: panes narrower than the wordmark
//!   render the one-row `▲ AbstractCode` brand line (never clipped
//!   glyph soup), and the element never flex-shrinks (`shrink(0.0)` —
//!   the 0240 class: a shrunk fixed row overprints its survivors).
//! - The animation is driven by a caller-owned frame signal — this
//!   module is PURE rendering (a `View` from `(tokens, frame)`), so the
//!   zero-wakeup policy stays where it belongs (the splash ticker in
//!   `ui::mod` arms only while the splash is visible).

use abstracttui::prelude::*;
use abstracttui::text;

/// The two wordmark rows (half-block letterforms, one space between
/// letters, three between the words). Both rows are the same display
/// width — pinned by a unit test, since the draw centers on it.
pub const WORD_TOP: &str = "▄▀█ █▄▄ █▀ ▀█▀ █▀█ ▄▀█ █▀▀ ▀█▀   █▀▀ █▀█ █▀▄ █▀▀";
pub const WORD_BOT: &str = "█▀█ █▄█ ▄█  █  █▀▄ █▀█ █▄▄  █    █▄▄ █▄█ █▄▀ ██▄";

/// The two-row `▲` mark (the header triangle at 2×: 4 lit cells) —
/// a single breathing cell was nearly subliminal (refinement pass).
pub const MARK_TOP: &str = " ▄ ";
pub const MARK_BOT: &str = "▄█▄";

/// One-row fallback for panes narrower than the wordmark.
const FALLBACK: &str = "▲ AbstractCode";

/// Total lockup rows (mark 2 + wordmark 2 + tagline 1). The tagline
/// lives INSIDE the lockup so the splash column spends no extra gap on
/// it — at 30-row terminals the whole identity block must fit with the
/// card AND the session-echo notices (the refinement pass's first cut
/// clipped a "buffered guidance dropped" echo off-screen, breaking the
/// never-a-silent-drop contract; caught by the session-boundary pin).
pub const LOGO_ROWS: i32 = 5;

/// The pulse's exact wrap period in frames: 36 frames = 5.4s at the
/// 150ms tick — exactly two breaths per 72-frame shimmer sweep
/// (harmonized 2:1, the combined loop repeats), and `frame % 36` never
/// meets the f32 integer-precision horizon a raw `frame as f32` sine
/// hits after ~29 days of idle (refinement pass, measured).
const PULSE_PERIOD: u64 = 36;

/// Channel-wise linear interpolation between two theme inks.
/// `t` is clamped to 0..=1; 0 = `a`, 1 = `b`.
pub fn lerp_ink(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    let ch = |x: u8, y: u8| -> u8 { (x as f32 + (y as f32 - x as f32) * t).round() as u8 };
    Rgba::new(ch(a.r, b.r), ch(a.g, b.g), ch(a.b, b.b), ch(a.a, b.a))
}

/// The shimmer weight for display column `col` at animation `frame`:
/// a triangular window (half-width 6 cols) around a sweep position that
/// crosses the wordmark once per `width + 2*PAD` frames (off-screen
/// padding both sides, so the sweep visibly enters and leaves instead
/// of wrapping mid-glyph). At ~150ms/frame a 48-col wordmark sweeps in
/// ~11s — present, never busy.
pub fn shimmer_weight(col: i32, width: i32, frame: u64) -> f32 {
    const PAD: i32 = 12;
    const HALF: f32 = 6.0;
    let period = (width + 2 * PAD).max(1) as u64;
    let pos = (frame % period) as i32 - PAD;
    let d = (col - pos).abs() as f32;
    (1.0 - d / HALF).clamp(0.0, 1.0)
}

/// The mark's breath at `frame`: an exact-wrap raised cosine, 0 at the
/// period boundary (boot reads as a fade-up), 1 mid-breath.
pub fn pulse_weight(frame: u64) -> f32 {
    let ph = (frame % PULSE_PERIOD) as f32 / PULSE_PERIOD as f32 * std::f32::consts::TAU;
    0.5 - 0.5 * ph.cos()
}

/// The animation's highlight inks with the SEPARATION FLOOR applied:
/// `(shimmer_hi, mark_hi)`. Two-stage walk, all theme-derived: the
/// accent walks toward `text` until it clears the floor against its
/// base (`text_muted` / `text_faint`); themes where even `text` cannot
/// separate from the base (measured: one-dark muted↔text at 1.16:1,
/// the catppuccin family, everforest-dark…) extend the walk toward the
/// theme's own luminance POLE (white on dark grounds, black on light —
/// pole choice derives from `t.bg`, never a hardcoded aesthetic).
/// Healthy themes return the accent unchanged.
pub fn floored_highlights(t: &TokenSet) -> (Rgba, Rgba) {
    use abstracttui::theme::contrast_ratio;
    use abstracttui::theme::derive::mix_until_contrast;
    // The theme's luminance pole: the direction separation always
    // exists in (cheap relative-luminance proxy; exactness is not
    // load-bearing — both poles separate, this just picks the one that
    // reads as "highlight" on this ground).
    let lum = |c: Rgba| 0.2126 * c.r as f32 + 0.7152 * c.g as f32 + 0.0722 * c.b as f32;
    let pole = if lum(t.bg) < 128.0 {
        Rgba::new(255, 255, 255, 255)
    } else {
        Rgba::new(0, 0, 0, 255)
    };
    let separated = |anchor: Rgba, floor: f32| -> Rgba {
        let first = mix_until_contrast(t.accent, t.text, anchor, 0.0, 0.15, floor);
        if contrast_ratio(first, anchor) >= floor {
            first
        } else {
            mix_until_contrast(first, pole, anchor, 0.0, 0.15, floor)
        }
    };
    (separated(t.text_muted, 1.35), separated(t.text_faint, 1.6))
}

/// The splash logo view: two mark rows + two wordmark rows + the
/// tagline (LOGO_ROWS; 1 centered row in the narrow fallback). Reads
/// NO signals — the caller passes the current frame and re-builds per
/// tick (the splash lives inside a dyn that already re-runs on the
/// frame signal).
pub fn logo(t: &TokenSet, frame: u64, tagline: &str) -> View {
    let word_w = text::width(WORD_TOP);
    let base = t.text_muted;
    let mark_lo = t.text_faint;
    let faint = t.text_faint;
    let tagline = tagline.to_string();
    let (hi, mark_hi) = floored_highlights(t);
    Element::new()
        // shrink(0.0): the brand block is the LAST casualty on short
        // panes, never the first (the refinement pass caught the logo
        // silently downgrading at 80×24 while card rows overprinted).
        .style(
            LayoutStyle::column()
                .height(Dimension::Cells(LOGO_ROWS))
                .shrink(0.0),
        )
        .draw(move |canvas, rect| {
            // Narrow fallback: the one-row brand line, breath-tinted,
            // vertically centered in the box (a top-pinned row left a
            // 2-row hole against the column's gap rhythm).
            if rect.w < word_w || rect.h < LOGO_ROWS {
                let ink = lerp_ink(mark_lo, mark_hi, pulse_weight(frame));
                let fitted = text::truncate_ellipsis(FALLBACK, rect.w.max(4));
                let x = rect.x + ((rect.w - text::width(&fitted)) / 2).max(0);
                let y = rect.y + ((rect.h - 1) / 2).max(0);
                canvas.print(Point::new(x, y), &fitted, ink, Rgba::TRANSPARENT);
                return;
            }
            let x0 = rect.x + ((rect.w - word_w) / 2).max(0);
            // Mark rows: the 2× triangle centered over the wordmark,
            // breathing between faint and the floored accent.
            let mark_ink = lerp_ink(mark_lo, mark_hi, pulse_weight(frame));
            let mark_w = text::width(MARK_TOP);
            let mx = rect.x + ((rect.w - mark_w) / 2).max(0);
            for (i, row) in [MARK_TOP, MARK_BOT].into_iter().enumerate() {
                canvas.print(
                    Point::new(mx, rect.y + i as i32),
                    row,
                    mark_ink,
                    Rgba::TRANSPARENT,
                );
            }
            // Wordmark rows: per-column shimmer. Half-block glyphs are
            // all width-1, so char index == display column.
            for (row_ix, row) in [WORD_TOP, WORD_BOT].into_iter().enumerate() {
                let y = rect.y + 2 + row_ix as i32;
                for (col, ch) in row.chars().enumerate() {
                    if ch == ' ' {
                        continue;
                    }
                    let w = shimmer_weight(col as i32, word_w, frame);
                    let ink = lerp_ink(base, hi, w);
                    canvas.print(
                        Point::new(x0 + col as i32, y),
                        &ch.to_string(),
                        ink,
                        Rgba::TRANSPARENT,
                    );
                }
            }
            // Tagline: faint, static (brand metadata completes the
            // lockup — animating it would compete with the wordmark).
            let fitted = text::truncate_ellipsis(&tagline, rect.w.max(4));
            let tx = rect.x + ((rect.w - text::width(&fitted)) / 2).max(0);
            canvas.print(
                Point::new(tx, rect.y + 4),
                &fitted,
                faint,
                Rgba::TRANSPARENT,
            );
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The draw centers on `WORD_TOP`'s width and paints both rows at
    /// the same origin — unequal widths would render a lopsided mark.
    /// Also pins that every glyph is width-1 (char index == column, the
    /// shimmer's addressing assumption), and the mark rows match.
    #[test]
    fn wordmark_rows_share_one_width_of_width_one_glyphs() {
        assert_eq!(text::width(WORD_TOP), text::width(WORD_BOT));
        for row in [WORD_TOP, WORD_BOT] {
            assert_eq!(
                row.chars().count() as i32,
                text::width(row),
                "every wordmark glyph must be display-width 1"
            );
        }
        assert_eq!(text::width(MARK_TOP), text::width(MARK_BOT));
        // Wide enough to be a wordmark, narrow enough for a 60-col pane.
        let w = text::width(WORD_TOP);
        assert!((40..=56).contains(&w), "wordmark width {w} out of band");
    }

    #[test]
    fn lerp_ink_hits_both_endpoints_and_clamps() {
        let a = Rgba::new(10, 20, 30, 255);
        let b = Rgba::new(210, 120, 90, 255);
        assert_eq!(lerp_ink(a, b, 0.0), a);
        assert_eq!(lerp_ink(a, b, 1.0), b);
        assert_eq!(lerp_ink(a, b, -1.0), a, "clamps below");
        assert_eq!(lerp_ink(a, b, 2.0), b, "clamps above");
        let mid = lerp_ink(a, b, 0.5);
        assert_eq!((mid.r, mid.g, mid.b), (110, 70, 60));
    }

    #[test]
    fn shimmer_sweeps_the_full_width_and_stays_bounded() {
        let width = 48;
        // Weight is always 0..=1.
        for frame in 0..200u64 {
            for col in 0..width {
                let w = shimmer_weight(col, width, frame);
                assert!((0.0..=1.0).contains(&w));
            }
        }
        // Every column peaks at SOME frame within one period (the sweep
        // genuinely crosses the whole wordmark; a stuck sweep dims it).
        let period = (width + 24) as u64;
        for col in 0..width {
            let peak = (0..period)
                .map(|f| shimmer_weight(col, width, f))
                .fold(0.0f32, f32::max);
            assert!(peak >= 0.99, "column {col} never lit (peak {peak})");
        }
    }

    /// The pulse: exact wrap (frame N and N+36 are byte-identical —
    /// no f32 precision horizon at any frame magnitude), a full 0..1
    /// band inside one period, and a fade-up start (0 at frame 0).
    #[test]
    fn pulse_wraps_exactly_and_breathes_the_full_band() {
        assert!(pulse_weight(0) < 0.01, "boot starts faint (fade-up)");
        let (mut lo, mut hi) = (1.0f32, 0.0f32);
        for f in 0..36u64 {
            let p = pulse_weight(f);
            assert!((0.0..=1.0).contains(&p));
            lo = lo.min(p);
            hi = hi.max(p);
            // Exact wrap even at magnitudes where `frame as f32` loses
            // integer precision (~2^24; measured 29 idle days) — the
            // modulo keeps the operand small forever.
            assert_eq!(p, pulse_weight(f + 36));
            assert_eq!(p, pulse_weight(f + 36 * 70_000_000));
        }
        assert!(lo < 0.01 && hi > 0.99, "pulse band [{lo}, {hi}] too narrow");
    }
}
