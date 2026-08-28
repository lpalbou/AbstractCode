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
use abstracttui::ui::StyledCanvas;

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

/// The wordmark's own view of [`sheen_weight`]: how lit display column
/// `col` is at `frame`. Kept as its own name because the compact lockup
/// and the audit suite both address the wordmark by column — but the
/// light is the SAME light the mark is under, so the two can never fall
/// out of step. At ~150 ms/frame a 48-column wordmark is crossed in
/// ~11 s of a ~14 s loop: present, never busy.
pub fn shimmer_weight(col: i32, width: i32, frame: u64) -> f32 {
    sheen_weight(col, 0, sheen_pos(frame, width))
}

/// ONE LIGHT over the whole lockup.
///
/// The mark and the wordmark used to shimmer on unrelated clocks, which
/// reads as two things twitching rather than one object being lit. Now a
/// single slightly-slanted band crosses the lockup — mark first, then the
/// letters — on a slow loop, and everything asks THIS function how lit it
/// is. Discreet by construction: one pass every ~14 s at the 150 ms
/// ticker, a soft cosine edge, and no motion of its own.
///
/// The band's leading edge is `x + y * SHEEN_SLANT`, so it leans like
/// light falling across a surface instead of wiping like a progress bar.
const SHEEN_SLANT: f32 = 0.45;
/// Half-width of the band in cells: wide enough that a 48-column
/// wordmark is never lit end to end, narrow enough to read as a pass.
const SHEEN_HALF: f32 = 9.0;
/// Cells the band travels beyond each edge, so it enters and leaves
/// instead of appearing mid-glyph.
const SHEEN_PAD: i32 = 14;

/// Where the light is at `frame`, in cells relative to the lockup's left
/// edge. Loops exactly: `width + 2 * SHEEN_PAD` frames per pass.
pub fn sheen_pos(frame: u64, width: i32) -> f32 {
    let period = (width + 2 * SHEEN_PAD).max(1) as u64;
    ((frame % period) as i32 - SHEEN_PAD) as f32
}

/// How lit the cell at `(x, y)` — both relative to the lockup's top-left
/// — is, given the band at `pos`. A raised cosine: no hard edge, no step.
pub fn sheen_weight(x: i32, y: i32, pos: f32) -> f32 {
    let u = x as f32 + y as f32 * SHEEN_SLANT;
    let d = (u - pos).abs() / SHEEN_HALF;
    if d >= 1.0 {
        0.0
    } else {
        // cos² falloff: 1 at the centre, 0 at the edge, flat at both.
        let k = (1.0 - d) * std::f32::consts::FRAC_PI_2;
        k.sin().powi(2)
    }
}

/// The mark's breath at `frame`: an exact-wrap raised cosine, 0 at the
/// period boundary (boot reads as a fade-up), 1 mid-breath.
pub fn pulse_weight(frame: u64) -> f32 {
    let ph = (frame % PULSE_PERIOD) as f32 / PULSE_PERIOD as f32 * std::f32::consts::TAU;
    0.5 - 0.5 * ph.cos()
}

/// The idle screen's ENTRANCE, in splash-ticker frames: the pane ramps
/// up from the ground instead of appearing whole. It shares the boot
/// animation's fade curve and lands in ~5 frames (~750 ms at the 150 ms
/// cadence), so the hand-off reads as one continuous arrival —
/// `ui::splash` fades its composition OUT to this same ground, and this
/// fades the app's first screen IN off it.
///
/// The ticker resets to frame 0 on every splash entrance, so a return to
/// the idle screen mid-session replays the same soft arrival.
pub fn boot_fade(frame: u64) -> f32 {
    const FRAMES: f32 = 5.0;
    let k = ((frame as f32 + 1.0) / FRAMES).clamp(0.0, 1.0);
    // Ease-out cubic: most of the ramp lands in the first two frames, so
    // a 150 ms cadence still reads as a fade rather than a slideshow.
    1.0 - (1.0 - k).powi(3)
}

/// Rows to give the HERO mark (the settled brand mark from the boot
/// animation) on a viewport of `h` rows — `None` when the pane cannot
/// afford it and the compact `▲` lockup should stand in.
///
/// The identity block is the FIRST thing in the idle column and the
/// column clips from the bottom, so every row spent here is a row the
/// fact card and the guidance lines do not get. These floors are the
/// heights at which the card still fits whole.
pub fn hero_rows(h: i32) -> Option<i32> {
    // Sized DOWN ~25% (operator, 2026-08-21: "the A is too big"): the
    // mark leads the lockup, it does not dominate the screen — at these
    // heights it reads as a mark over a wordmark rather than a poster.
    match h {
        44.. => Some(9),
        38..=43 => Some(8),
        34..=37 => Some(6),
        _ => None,
    }
}

/// Total rows a hero lockup occupies: mark + wordmark (2) + tagline.
pub fn hero_lockup_rows(mark_rows: i32) -> i32 {
    mark_rows + 3
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
    // The shimmer floor is deliberately HIGH (2026-08-21: the sweep was
    // there but nobody could see it on the house theme). The walk
    // saturates at the theme's own pole, so themes that cannot separate
    // further simply land where they always did — this raises the
    // ceiling, it never invents a color the theme does not own.
    (separated(t.text_muted, 1.9), separated(t.text_faint, 1.6))
}

/// The splash logo view: two mark rows + two wordmark rows + the
/// tagline (LOGO_ROWS; 1 centered row in the narrow fallback). Reads
/// NO signals — the caller passes the current frame and re-builds per
/// tick (the splash lives inside a dyn that already re-runs on the
/// frame signal). `fade` is the entrance ramp ([`boot_fade`]): every ink
/// here rides it, so the compact lockup arrives exactly like the hero
/// one does.
pub fn logo(t: &TokenSet, frame: u64, tagline: &str, fade: f32) -> View {
    let word_w = text::width(WORD_TOP);
    let ground = t.bg;
    let dim = move |c: Rgba| lerp_ink(ground, c, fade);
    let base = dim(t.text_muted);
    let mark_lo = dim(t.text_faint);
    let faint = dim(t.text_faint);
    let tagline = tagline.to_string();
    let (hi, mark_hi) = floored_highlights(t);
    let (hi, mark_hi) = (dim(hi), dim(mark_hi));
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
            // The light's clock is the WORDMARK's width, not the
            // pane's: one pass every ~11 s on a 60-column terminal and
            // on a 200-column one alike (a pane-width period made the
            // same animation twice as slow on a wide screen).
            let pos = sheen_pos(frame, word_w);
            let light = |x: i32, y: i32| sheen_weight(x - x0, y - rect.y, pos);
            draw_wordmark(canvas, x0, rect.y + 2, base, hi, &light);
            draw_tagline(canvas, rect, rect.y + 4, &tagline, faint);
        })
        .build()
}

/// The two half-block wordmark rows at `(x0, y)`, lit by the lockup's
/// ONE light (`light` maps an absolute cell to 0..=1). Half-block glyphs
/// are all width-1, so char index == display column. One implementation:
/// the compact lockup and the hero lockup draw the wordmark through
/// this, so the two can never disagree about the art OR the light.
fn draw_wordmark(
    canvas: &mut dyn StyledCanvas,
    x0: i32,
    y: i32,
    base: Rgba,
    hi: Rgba,
    light: &dyn Fn(i32, i32) -> f32,
) {
    for (row_ix, row) in [WORD_TOP, WORD_BOT].into_iter().enumerate() {
        let y = y + row_ix as i32;
        for (col, ch) in row.chars().enumerate() {
            if ch == ' ' {
                continue;
            }
            let x = x0 + col as i32;
            let ink = lerp_ink(base, hi, light(x, y));
            canvas.print(Point::new(x, y), &ch.to_string(), ink, Rgba::TRANSPARENT);
        }
    }
}

/// Tagline: faint, static (brand metadata completes the lockup —
/// animating it would compete with the wordmark), centered and
/// ellipsized to the pane.
fn draw_tagline(canvas: &mut dyn StyledCanvas, rect: Rect, y: i32, tagline: &str, ink: Rgba) {
    let fitted = text::truncate_ellipsis(tagline, rect.w.max(4));
    let x = rect.x + ((rect.w - text::width(&fitted)) / 2).max(0);
    canvas.print(Point::new(x, y), &fitted, ink, Rgba::TRANSPARENT);
}

/// The HERO lockup: the boot animation's own mark, at rest, over the
/// same wordmark and tagline — so the first screen after the animation
/// carries the letterform the animation just assembled instead of a
/// second, smaller mark that merely rhymes with it.
///
/// `mark_rows` comes from [`hero_rows`] (None = this pane cannot afford
/// it, use [`logo`]); `fade` is the entrance ramp from [`boot_fade`].
/// Same contract as [`logo`]: pure rendering, no signal reads.
pub fn hero(t: &TokenSet, frame: u64, tagline: &str, mark_rows: i32, fade: f32) -> View {
    let word_w = text::width(WORD_TOP);
    let ground = t.bg;
    let dim = move |c: Rgba| lerp_ink(ground, c, fade);
    let base = dim(t.text_muted);
    let faint = dim(t.text_faint);
    let (hi_raw, _) = floored_highlights(t);
    let hi = dim(hi_raw);
    let tagline = tagline.to_string();
    let rows = hero_lockup_rows(mark_rows);
    Element::new()
        .style(
            LayoutStyle::column()
                .height(Dimension::Cells(rows))
                .shrink(0.0),
        )
        .draw(move |canvas, rect| {
            if rect.w < word_w || rect.h < rows {
                return; // the caller sizes this; a clipped hero is not a hero
            }
            // ONE light for the whole lockup: a slow, slightly slanted
            // band that crosses the mark and the letters as a single
            // pass. The mark also breathes on the compact lockup's pulse
            // underneath it — a lift through the brand ramp, not a blink.
            let x0 = rect.x + ((rect.w - word_w) / 2).max(0);
            let pos = sheen_pos(frame, word_w);
            let light = |x: i32, y: i32| sheen_weight(x - x0, y - rect.y, pos);
            let mark_w = (mark_rows as f32 * 2.0 * 0.75) as i32;
            let mark = Rect::new(
                rect.x + ((rect.w - mark_w) / 2).max(0),
                rect.y,
                mark_w.min(rect.w),
                mark_rows,
            );
            crate::ui::splash::draw_settled_mark(
                canvas,
                mark,
                ground,
                fade,
                pulse_weight(frame),
                &light,
            );
            draw_wordmark(canvas, x0, rect.y + mark_rows, base, hi, &light);
            draw_tagline(canvas, rect, rect.y + mark_rows + 2, &tagline, faint);
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

    /// The light is ONE light: the mark and the wordmark must read the
    /// same band at the same instant, or the lockup twitches in two
    /// places instead of being lit in one.
    #[test]
    fn the_sheen_is_one_band_with_a_soft_edge() {
        let pos = sheen_pos(30, 48);
        // Centre is fully lit, the edges fall to nothing, and nothing
        // outside the band is lit at all.
        let at = |x: i32, y: i32| sheen_weight(x, y, pos);
        let peak = at(pos as i32, 0);
        assert!(peak > 0.99, "the band's centre is fully lit ({peak})");
        assert_eq!(at(pos as i32 + 20, 0), 0.0, "outside the band: dark");
        assert_eq!(at(pos as i32 - 20, 0), 0.0, "outside the band: dark");
        // Soft edge: monotone falloff, no step.
        let mut prev = peak;
        for d in 1..9 {
            let w = at(pos as i32 + d, 0);
            assert!(w <= prev, "falloff is monotone at +{d} ({w} > {prev})");
            prev = w;
        }
        // The band LEANS: a lower row is reached later, which is what
        // makes it read as light raking across rather than a wipe.
        assert!(
            at(pos as i32, 4) < at(pos as i32, 0),
            "the slant delays lower rows"
        );
        // It loops exactly, with no jump at the wrap.
        assert_eq!(sheen_pos(0, 48), sheen_pos(48 + 2 * SHEEN_PAD as u64, 48));
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
