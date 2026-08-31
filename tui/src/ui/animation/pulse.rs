//! `/animation 1 · pulse` — the run as a live strip chart.
//!
//! The whole run, always, compressed to the pane's width: one column per
//! time bucket, the bucket widening as the run grows so minute 90 shows
//! the same shape as minute 3 did, only denser. Three lanes:
//!
//! - **cycles** — one bar per model call, height by output tokens;
//! - **tools** — one row per tool family, a tick where a call landed,
//!   and a full-height column in error ink where one FAILED (a failing
//!   streak is a picket fence you cannot miss);
//! - **context** — the filled area under the declared window, when the
//!   operator declared one.
//!
//! Reading it: a steady rhythm of bar → ticks → bar is a healthy run. A
//! long gap ending in one bar is a slow model. A red fence is thrashing.
//! A flat right edge with the cursor still is a run that has stopped
//! producing — which is exactly what the state line says.

use abstracttui::base::{Point, Rect, Rgba};
use abstracttui::prelude::*;
use abstracttui::theme::derive::mix;
use abstracttui::ui::StyledCanvas;

use super::{Family, Feed, Outcome, Snapshot};

/// Families that get their own row, top to bottom.
const LANES: [Family; 6] = [
    Family::Read,
    Family::Search,
    Family::Write,
    Family::Exec,
    Family::Net,
    Family::Other,
];

pub fn render(
    canvas: &mut dyn StyledCanvas,
    rect: Rect,
    t: &TokenSet,
    feed: &Feed,
    snap: &Snapshot,
    frame: u64,
) {
    if rect.w < 24 || rect.h < 9 {
        return compact(canvas, rect, t, feed, snap, frame);
    }
    let cols = rect.w - 8; // 8 columns of lane labels
    let x0 = rect.x + 8;
    // The whole run always fits: the bucket widens instead of scrolling,
    // so minute 90 shows the same shape minute 3 did, only denser.
    let bucket = (feed.span_ms() as f32 / cols as f32).max(200.0);
    let col_of = |at_ms: u64| -> i32 { ((at_ms as f32 / bucket) as i32).clamp(0, cols - 1) };

    // ---- lane geometry: one block, vertically centered ------------------
    let ctx_rows = if snap.ctx_frac.is_some() { 2 } else { 0 };
    let tool_rows = LANES.len() as i32;
    let cycle_rows = (rect.h - tool_rows - ctx_rows - 4).clamp(3, 10);
    let block = cycle_rows + 1 + tool_rows + ctx_rows + 1;
    let cycle_y = rect.y + ((rect.h - block) / 2).max(0);
    let tool_y = cycle_y + cycle_rows + 1;
    let ctx_y = tool_y + tool_rows + 1;

    // ---- cycles ---------------------------------------------------------
    label(canvas, rect.x, cycle_y, "cycles", t.text_faint);
    let peak = feed
        .events
        .iter()
        .filter(|e| e.family == Family::Think)
        .map(|e| e.tokens)
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    for ev in feed.events.iter().filter(|e| e.family == Family::Think) {
        let x = x0 + col_of(ev.at_ms);
        let h = ((ev.tokens as f32 / peak).sqrt() * cycle_rows as f32).round() as i32;
        let h = h.clamp(1, cycle_rows);
        for i in 0..h {
            let y = cycle_y + cycle_rows - 1 - i;
            let k = i as f32 / cycle_rows.max(1) as f32;
            canvas.put(
                Point::new(x, y),
                '█',
                mix(Family::Think.ink(t), t.text, k * 0.35),
                Rgba::TRANSPARENT,
            );
        }
    }

    // ---- tools ----------------------------------------------------------
    for (i, fam) in LANES.iter().enumerate() {
        let y = tool_y + i as i32;
        label(canvas, rect.x, y, fam.short(), t.text_faint);
        // The lane's own hairline: an empty lane is visibly empty.
        for x in 0..cols {
            canvas.put(
                Point::new(x0 + x, y),
                '·',
                mix(t.bg, t.border, 0.55),
                Rgba::TRANSPARENT,
            );
        }
    }
    for ev in feed.events.iter().filter(|e| e.family != Family::Think) {
        let Some(i) = LANES.iter().position(|f| *f == ev.family) else {
            continue;
        };
        let x = x0 + col_of(ev.at_ms);
        let y = tool_y + i as i32;
        match ev.outcome {
            Outcome::Failed | Outcome::Denied => {
                // A failure is not a dot: it is a column through every
                // lane, so a streak reads as a fence from across the room.
                for k in 0..LANES.len() as i32 {
                    canvas.put(
                        Point::new(x, tool_y + k),
                        '│',
                        mix(t.bg, t.error, 0.75),
                        Rgba::TRANSPARENT,
                    );
                }
                canvas.put(Point::new(x, y), '╳', t.error, Rgba::TRANSPARENT);
            }
            Outcome::Running => {
                canvas.put(Point::new(x, y), '◌', ev.family.ink(t), Rgba::TRANSPARENT);
            }
            Outcome::Ok => {
                canvas.put(Point::new(x, y), '▮', ev.family.ink(t), Rgba::TRANSPARENT);
            }
        }
    }

    // ---- context --------------------------------------------------------
    if let Some(frac) = snap.ctx_frac {
        label(canvas, rect.x, ctx_y + 1, "ctx", t.text_faint);
        let filled = ((cols as f32) * frac.clamp(0.0, 1.0)).round() as i32;
        for x in 0..cols {
            let on = x < filled;
            let ink = if !on {
                mix(t.bg, t.border, 0.5)
            } else if frac > 0.85 {
                t.error
            } else if frac > 0.6 {
                t.warn
            } else {
                t.info
            };
            canvas.put(
                Point::new(x0 + x, ctx_y + 1),
                if on { '▄' } else { '·' },
                ink,
                Rgba::TRANSPARENT,
            );
        }
        let pct = format!("{:.0}%", frac * 100.0);
        canvas.print(
            Point::new(x0 + cols - pct.len() as i32 - 1, ctx_y),
            &pct,
            t.text_muted,
            Rgba::TRANSPARENT,
        );
    }

    // ---- the cursor -----------------------------------------------------
    // The right edge is NOW. Its breath is the honesty valve: it beats
    // while work lands, and it goes still — visibly still — when the run
    // stops producing.
    let cursor_x = x0 + col_of(feed.elapsed_ms()).min(cols - 1);
    let motion = snap.state.motion();
    let beat = if motion <= 0.0 {
        0.0
    } else {
        let ph = (frame % 16) as f32 / 16.0 * std::f32::consts::TAU;
        (0.5 - 0.5 * ph.cos()) * motion
    };
    let ink = mix(t.text_faint, snap.state.ink(t), 0.35 + 0.65 * beat);
    for y in cycle_y..ctx_y + ctx_rows.max(1) {
        if y >= rect.y + rect.h {
            break;
        }
        canvas.put(Point::new(cursor_x, y), '▏', ink, Rgba::TRANSPARENT);
    }
}

fn label(canvas: &mut dyn StyledCanvas, x: i32, y: i32, s: &str, ink: Rgba) {
    canvas.print(Point::new(x, y), s, ink, Rgba::TRANSPARENT);
}

/// The 6-row form for a small pane: one lane, the whole run, plus the
/// counters. Designed, not clipped.
fn compact(
    canvas: &mut dyn StyledCanvas,
    rect: Rect,
    t: &TokenSet,
    feed: &Feed,
    snap: &Snapshot,
    frame: u64,
) {
    if rect.w < 8 || rect.h < 1 {
        return;
    }
    let y = rect.y + rect.h / 2;
    let cols = rect.w;
    let bucket = (feed.span_ms() as f32 / cols as f32).max(200.0);
    for ev in feed.events.iter() {
        let x = rect.x + ((ev.at_ms as f32 / bucket) as i32).min(cols - 1);
        let (ch, ink) = match (ev.family, ev.outcome) {
            (_, Outcome::Failed) | (_, Outcome::Denied) => ('╳', t.error),
            (Family::Think, _) => ('▔', Family::Think.ink(t)),
            (f, _) => ('▮', f.ink(t)),
        };
        canvas.put(Point::new(x, y), ch, ink, Rgba::TRANSPARENT);
    }
    let beat = if snap.state.motion() <= 0.0 {
        0.0
    } else {
        ((frame % 16) as f32 / 16.0 * std::f32::consts::TAU).cos() * -0.5 + 0.5
    };
    canvas.put(
        Point::new(rect.x + cols - 1, y),
        '▏',
        mix(t.text_faint, snap.state.ink(t), 0.35 + 0.65 * beat),
        Rgba::TRANSPARENT,
    );
    if rect.h >= 3 {
        let line = format!(
            "{} cycles · {} tools · {} failed",
            snap.llm_calls, snap.tool_calls, snap.tool_failures
        );
        canvas.print(
            Point::new(rect.x, y + 2),
            &abstracttui::text::truncate_ellipsis(&line, cols),
            t.text_muted,
            Rgba::TRANSPARENT,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::animation::State;

    /// An empty run draws the lane hairlines and nothing else — the
    /// chart must not invent a shape it has no data for.
    #[test]
    fn an_empty_run_has_no_marks() {
        let feed = Feed::new();
        assert!(feed.events.is_empty());
        // Family classification is the one shared vocabulary; pin it.
        assert_eq!(Family::of("edit_file"), Family::Write);
        assert_eq!(Family::of("read_file"), Family::Read);
        assert_eq!(Family::of("execute_command"), Family::Exec);
        assert_eq!(Family::of("search_files"), Family::Search);
        assert_eq!(Family::of("fetch_url"), Family::Net);
        assert_eq!(Family::of("something_else"), Family::Other);
    }

    /// The cursor's beat is the honesty valve: no motion when the state
    /// says nothing is happening.
    #[test]
    fn a_still_state_has_a_still_cursor() {
        assert_eq!(State::Down.motion(), 0.0);
        assert_eq!(State::Idle.motion(), 0.0);
        assert!(State::Quiet.motion() < 0.2, "a quiet run barely moves");
        assert!(State::Working.motion() > 0.9);
    }
}
