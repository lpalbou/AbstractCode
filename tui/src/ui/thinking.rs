//! The working indicator: a colored block wave that replaces the engine
//! `Spinner`'s single braille dot on the activity strip.
//!
//! Why a wave: the spinner is one cell in `accent` and reads as
//! punctuation next to its label — on a busy screen it is easy to miss
//! that the agent is working at all (operator report, 2026-08-19: make
//! it noticeable). Six cells that move in HEIGHT and in INK carry the
//! same fact at a glance, and motion in two channels survives both
//! low-contrast themes and peripheral vision.
//!
//! Design rules, inherited from `ui::logo` deliberately:
//! - THEME INKS ONLY, with the same SEPARATION FLOOR: the crest ink is
//!   `logo::floored_highlights(&t).1` over a `text_faint` trough — the
//!   exact pair `tests/theme_contrast_audit.rs` already pins at ≥1.5:1
//!   on every registry theme, so a registry drift that dims this wave
//!   fails that audit rather than shipping an invisible animation.
//! - EXACT-WRAP period: the phase is `frame % WAVE_PERIOD`, never a raw
//!   `frame as f32`, which loses integer precision after ~29 days of
//!   ticking (the `logo::PULSE_PERIOD` rule, measured there).
//! - PURE rendering: the caller owns the frame signal, so the
//!   zero-wakeup policy stays with the ticker in `ui::mod` (armed only
//!   while a run or an entity turn is live).
//! - HONEST DEGRADATION: the wave never flex-shrinks (the 0240 class —
//!   a shrunk fixed row overprints its survivors) and draws only the
//!   cells that fit, so a narrow pane loses wave, not label.

use abstracttui::prelude::*;

use crate::ui::logo::{floored_highlights, lerp_ink};

/// Wave cells. Six reads as a wave at a glance; four read as a dotted
/// line and eight crowd the label on 80-column terminals.
pub const WAVE_CELLS: usize = 6;

/// Frames for one full travel of the crest. 20 at the strip ticker's
/// 120ms = 2.4s — present and legible, not a strobe.
pub const WAVE_PERIOD: u64 = 20;

/// Lower-eighth blocks: eight height steps in one narrow cell. Block
/// elements are the safe family here (the wordmark already ships them);
/// round glyphs like `●` are East-Asian AMBIGUOUS and double-print on
/// terminals configured for wide ambiguity.
const GLYPHS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Height/ink weight of `col` at `frame`: a raised cosine with a
/// per-column phase offset, so the crest travels along the wave instead
/// of every cell breathing together. 0 = trough, 1 = crest.
pub fn cell_weight(col: usize, frame: u64) -> f32 {
    let phase = (frame % WAVE_PERIOD) as f32 / WAVE_PERIOD as f32;
    let offset = col as f32 / WAVE_CELLS as f32;
    // MINUS: the crest travels left→right, the direction progress reads
    // in (a right→left crest reads as rewinding).
    let ph = (phase - offset) * std::f32::consts::TAU;
    0.5 - 0.5 * ph.cos()
}

/// Glyph for a weight: index into the eight block heights.
pub fn glyph_for(weight: f32) -> char {
    let ix = (weight.clamp(0.0, 1.0) * (GLYPHS.len() - 1) as f32).round() as usize;
    GLYPHS[ix.min(GLYPHS.len() - 1)]
}

/// The indicator: `WAVE_CELLS` animated cells, a gap, then `label`.
/// Grows to fill its row so the label gets the remaining width.
pub fn element(t: &TokenSet, frame: u64, label: String) -> Element {
    let trough = t.text_faint;
    let crest = floored_highlights(t).1;
    let label_fg = t.text_muted;
    Element::new()
        .style(LayoutStyle::default().h(1).grow(1.0).shrink(0.0))
        .draw(move |canvas, rect| {
            if rect.w <= 0 || rect.h <= 0 {
                return;
            }
            let cells = WAVE_CELLS.min(rect.w.max(0) as usize);
            for col in 0..cells {
                let w = cell_weight(col, frame);
                canvas.put(
                    Point::new(rect.x + col as i32, rect.y),
                    glyph_for(w),
                    lerp_ink(trough, crest, w),
                    Rgba::TRANSPARENT,
                );
            }
            let label_x = rect.x + cells as i32 + 1;
            let avail = rect.right() - label_x;
            if avail > 0 && !label.is_empty() {
                let fitted = abstracttui::text::truncate_ellipsis(&label, avail);
                canvas.print(
                    Point::new(label_x, rect.y),
                    &fitted,
                    label_fg,
                    Rgba::TRANSPARENT,
                );
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wave_travels_and_wraps_exactly() {
        // Every column reaches both ends of its range within one period
        // (a wave that never crests is an invisible animation).
        for col in 0..WAVE_CELLS {
            let ws: Vec<f32> = (0..WAVE_PERIOD).map(|f| cell_weight(col, f)).collect();
            let lo = ws.iter().cloned().fold(f32::INFINITY, f32::min);
            let hi = ws.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            assert!(lo < 0.1, "col {col} never troughs: {lo}");
            assert!(hi > 0.9, "col {col} never crests: {hi}");
        }
        // Exact wrap: frame N and frame N+PERIOD are the same picture,
        // and the far future is identical to the first period (no f32
        // precision horizon — the logo::PULSE_PERIOD rule).
        for col in 0..WAVE_CELLS {
            assert_eq!(cell_weight(col, 3), cell_weight(col, 3 + WAVE_PERIOD));
            assert_eq!(
                cell_weight(col, 7),
                cell_weight(col, 7 + WAVE_PERIOD * 1_000_000)
            );
        }
    }

    #[test]
    fn neighbouring_cells_differ_so_the_crest_is_visible() {
        // A phase offset that collapsed to zero would breathe as one
        // block — the shape half of the signal would be gone.
        let frame = 0;
        let a = cell_weight(0, frame);
        let b = cell_weight(WAVE_CELLS / 2, frame);
        assert!((a - b).abs() > 0.5, "crest does not travel: {a} vs {b}");
    }

    #[test]
    fn glyphs_cover_both_extremes_and_stay_in_range() {
        assert_eq!(glyph_for(0.0), '▁');
        assert_eq!(glyph_for(1.0), '█');
        // Out-of-range weights clamp rather than panic on the index.
        assert_eq!(glyph_for(-5.0), '▁');
        assert_eq!(glyph_for(5.0), '█');
    }
}
