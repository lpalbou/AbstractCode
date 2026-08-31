//! `/animation 2 · desk` — the intern: a pet whose every prop is a real
//! number.
//!
//! The objection to a coding-tool mascot is not that it is silly, it is
//! that it is decoupled from the truth: it keeps bouncing while the run
//! is dead. So nothing here is decorative —
//!
//! - the **posture** is the current activity (typing on a write, reading
//!   a sheet on a read, staring at the ceiling while the model thinks,
//!   holding a sign while an approval waits, asleep when idle);
//! - the **typing speed** is the model's measured tokens per second, so
//!   a slow local model visibly grinds;
//! - the **paper stack** is the context window filling up;
//! - the **coffee cups** are model calls;
//! - the **bin** is failed tool calls;
//! - the **clock** is the run's elapsed time;
//! - and when the run stops producing, the intern stops moving. It does
//!   not mime work that is not happening.
//!
//! Hover-free by design: the status row under the scene names the state
//! in words, so the joke is never the only carrier of a fact.

use abstracttui::base::{Point, Rect, Rgba};
use abstracttui::prelude::*;
use abstracttui::theme::derive::mix;
use abstracttui::ui::StyledCanvas;

use super::{Family, Feed, Outcome, Snapshot, State};

/// What the figure is doing this frame — derived, never chosen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pose {
    Typing,
    Reading,
    Thinking,
    Signing,
    Sleeping,
    Facepalm,
}

fn pose_of(feed: &Feed, snap: &Snapshot) -> Pose {
    match snap.state {
        State::Waiting => Pose::Signing,
        State::Idle => Pose::Sleeping,
        State::Failing => Pose::Facepalm,
        State::Down => Pose::Sleeping,
        _ => match feed.events.back().map(|e| e.family) {
            Some(Family::Write) | Some(Family::Exec) => Pose::Typing,
            Some(Family::Read) | Some(Family::Search) | Some(Family::Net) => Pose::Reading,
            _ => Pose::Thinking,
        },
    }
}

/// The figure, five rows, per pose. Column 0 of each row is the left
/// edge of the sprite box (7 cells wide).
fn sprite(pose: Pose, tick: bool) -> [&'static str; 5] {
    match pose {
        Pose::Typing if tick => ["  ___  ", " (o o) ", " /|_|\\ ", "  | |  ", " _/ \\_ "],
        Pose::Typing => ["  ___  ", " (o o) ", " /|=|\\ ", "  | |  ", " _/ \\_ "],
        Pose::Reading => ["  ___  ", " (- o) ", " /|_|\\ ", " [| |] ", " _/ \\_ "],
        Pose::Thinking if tick => ["  ___  ", " (^ ^) ", " \\|_|/ ", "  | |  ", " _/ \\_ "],
        Pose::Thinking => ["  ___  ", " (o ^) ", " \\|_|/ ", "  | |  ", " _/ \\_ "],
        Pose::Signing => ["  ___  ", " (O O) ", " /|_|\\ ", "  | |  ", " _/ \\_ "],
        Pose::Sleeping => ["  ___  ", " (- -) ", " /|_|\\ ", "  |_|  ", " _/ \\_ "],
        Pose::Facepalm => ["  ___  ", " (x x) ", " \\|_|/ ", "  |o|  ", " _/ \\_ "],
    }
}

/// The scene's natural size. Fixed, centered: a diorama that stretches
/// with the pane stops reading as a room.
const SCENE_W: i32 = 62;
const SCENE_H: i32 = 15;

pub fn render(
    canvas: &mut dyn StyledCanvas,
    rect: Rect,
    t: &TokenSet,
    feed: &Feed,
    snap: &Snapshot,
    frame: u64,
) {
    if rect.w < SCENE_W || rect.h < SCENE_H {
        return compact(canvas, rect, t, snap, frame);
    }
    let pose = pose_of(feed, snap);
    let ink = t.text_muted;
    let faint = t.text_faint;
    let moving = snap.state.motion() > 0.0;

    // One centered room, always the same size.
    let s = Rect::new(
        rect.x + (rect.w - SCENE_W) / 2,
        rect.y + (rect.h - SCENE_H) / 2,
        SCENE_W,
        SCENE_H,
    );
    let floor = s.y + SCENE_H - 2;
    let desk_y = floor - 2;

    // ---- the room: a wall, a floor line ---------------------------------
    // A tinted rectangle is what makes the diorama read as a ROOM rather
    // than glyphs floating on the app's ground.
    let wall = mix(t.bg, t.surface_raised, 0.55);
    let ground = mix(t.bg, t.surface, 0.9);
    for y in 0..SCENE_H {
        for x in 0..SCENE_W {
            let c = if s.y + y > floor { ground } else { wall };
            canvas.put(Point::new(s.x + x, s.y + y), ' ', c, c);
        }
    }
    for x in 0..SCENE_W {
        canvas.put(
            Point::new(s.x + x, floor),
            '─',
            mix(t.bg, t.border, 0.9),
            Rgba::TRANSPARENT,
        );
    }

    // ---- the poster on the wall: the house mark ------------------------
    canvas.print(
        Point::new(s.x + 2, s.y + 1),
        " ▄ ",
        faint,
        Rgba::TRANSPARENT,
    );
    canvas.print(
        Point::new(s.x + 2, s.y + 2),
        "▄█▄",
        faint,
        Rgba::TRANSPARENT,
    );

    // ---- the clock on the wall -----------------------------------------
    let clock = format!(
        "{:02}:{:02}:{:02}",
        snap.elapsed_secs / 3600,
        (snap.elapsed_secs / 60) % 60,
        snap.elapsed_secs % 60
    );
    let cx = s.x + SCENE_W - clock.chars().count() as i32 - 2;
    canvas.print(
        Point::new(cx, s.y + 1),
        "┌────────┐",
        faint,
        Rgba::TRANSPARENT,
    );
    canvas.print(Point::new(cx, s.y + 2), &clock, ink, Rgba::TRANSPARENT);
    canvas.print(
        Point::new(cx, s.y + 3),
        "└────────┘",
        faint,
        Rgba::TRANSPARENT,
    );

    // ---- the monitor: what the run is doing, in the app's own words -----
    let mon_x = s.x + 24;
    let mon_y = s.y + 1;
    let screen_ink = match snap.state {
        State::Failing => t.error,
        State::Waiting => t.warn,
        State::Idle | State::Down => faint,
        _ => Family::Think.ink(t),
    };
    canvas.print(
        Point::new(mon_x, mon_y),
        "┌──────────────────┐",
        ink,
        Rgba::TRANSPARENT,
    );
    for r in 1..4 {
        canvas.print(
            Point::new(mon_x, mon_y + r),
            "│                  │",
            ink,
            Rgba::TRANSPARENT,
        );
    }
    canvas.print(
        Point::new(mon_x, mon_y + 4),
        "└───────┬──────────┘",
        ink,
        Rgba::TRANSPARENT,
    );
    canvas.print(
        Point::new(mon_x + 6, mon_y + 5),
        "───┴───",
        ink,
        Rgba::TRANSPARENT,
    );
    // Line 1: the newest tool and its file (already sanitized).
    if let Some(ev) = feed.events.iter().rev().find(|e| e.family != Family::Think) {
        let tag = format!("{} {}", ev.family.short(), ev.label);
        canvas.print(
            Point::new(mon_x + 2, mon_y + 1),
            &abstracttui::text::truncate_ellipsis(&tag, 16),
            match ev.outcome {
                Outcome::Failed | Outcome::Denied => t.error,
                _ => ev.family.ink(t),
            },
            Rgba::TRANSPARENT,
        );
    }
    // Line 2: a cursor that types while the model generates. Its cadence
    // IS the measured rate — a 3 tok/s model grinds, a fast one blurs.
    let rate = snap.tok_per_s.unwrap_or(0.0);
    let period = if rate <= 0.0 {
        8
    } else {
        (60.0 / rate.clamp(1.0, 60.0)).clamp(1.0, 10.0) as u64
    };
    let tick = moving && (frame / period.max(1)).is_multiple_of(2);
    let bar: String = if moving {
        let n = ((frame / period.max(1)) % 14) as usize;
        "▖".repeat(n.min(14))
    } else {
        String::new()
    };
    canvas.print(
        Point::new(mon_x + 2, mon_y + 2),
        &bar,
        screen_ink,
        Rgba::TRANSPARENT,
    );
    if tick {
        canvas.put(
            Point::new(mon_x + 2 + bar.chars().count() as i32, mon_y + 2),
            '▌',
            screen_ink,
            Rgba::TRANSPARENT,
        );
    }
    canvas.print(
        Point::new(mon_x + 2, mon_y + 3),
        &abstracttui::text::truncate_ellipsis(snap.state.label(), 16),
        snap.state.ink(t),
        Rgba::TRANSPARENT,
    );

    // ---- the desk -------------------------------------------------------
    let desk_x0 = s.x + 14;
    let desk_x1 = s.x + SCENE_W - 12;
    for x in desk_x0..desk_x1 {
        canvas.put(Point::new(x, desk_y), '━', ink, Rgba::TRANSPARENT);
    }
    for x in [desk_x0 + 1, desk_x1 - 2] {
        canvas.put(Point::new(x, desk_y + 1), '┃', faint, Rgba::TRANSPARENT);
    }

    // ---- the figure -----------------------------------------------------
    let fig_x = s.x + 16;
    let fig_y = desk_y - 5;
    let fig_ink = match snap.state {
        State::Failing => t.error,
        State::Waiting => t.warn,
        State::Idle | State::Down => faint,
        _ => t.text,
    };
    for (i, row) in sprite(pose, tick).iter().enumerate() {
        canvas.print(
            Point::new(fig_x, fig_y + i as i32),
            row,
            fig_ink,
            Rgba::TRANSPARENT,
        );
    }
    match pose {
        Pose::Signing => {
            // Held UP, over the figure's head — never across the desk,
            // where it would cover the numbers it is asking about.
            canvas.print(
                Point::new(fig_x - 2, fig_y - 3),
                "┌──────────┐",
                t.warn,
                Rgba::TRANSPARENT,
            );
            canvas.print(
                Point::new(fig_x - 2, fig_y - 2),
                "│ APPROVE? │",
                t.warn,
                Rgba::TRANSPARENT,
            );
            canvas.print(
                Point::new(fig_x - 2, fig_y - 1),
                "└────┬─────┘",
                t.warn,
                Rgba::TRANSPARENT,
            );
        }
        Pose::Sleeping => {
            for (i, z) in ["z", "Z", "z"].iter().enumerate() {
                let up = (frame / 6) as i32 % 3;
                canvas.print(
                    Point::new(fig_x + 7 + i as i32, fig_y - up + i as i32),
                    z,
                    faint,
                    Rgba::TRANSPARENT,
                );
            }
        }
        Pose::Thinking if moving => {
            for i in 0..3u64 {
                let ph = (frame / 3 + i * 2) % 6;
                canvas.put(
                    Point::new(fig_x + 7 + i as i32, fig_y - 1 - ph as i32 / 2),
                    '·',
                    mix(t.bg, Family::Think.ink(t), 1.0 - ph as f32 / 6.0),
                    Rgba::TRANSPARENT,
                );
            }
        }
        _ => {}
    }

    // ---- the paper: the context window ----------------------------------
    let (stack_units, stack_ink, stack_note) = match snap.ctx_frac {
        Some(f) => (
            (f * 10.0).round() as i32,
            if f > 0.85 {
                t.error
            } else if f > 0.6 {
                t.warn
            } else {
                ink
            },
            format!("ctx {:.0}%", f * 100.0),
        ),
        None => (
            (snap.llm_calls as i32 / 3).min(10),
            ink,
            format!("{} cycles", snap.llm_calls),
        ),
    };
    let stack_x = s.x + 4;
    for i in 0..stack_units.max(0) {
        let y = floor - 1 - i;
        if y <= s.y + 4 {
            break;
        }
        canvas.print(
            Point::new(stack_x, y),
            "▄▄▄▄▄",
            stack_ink,
            Rgba::TRANSPARENT,
        );
    }
    canvas.print(
        Point::new(stack_x, floor + 1),
        &stack_note,
        faint,
        Rgba::TRANSPARENT,
    );

    // ---- the cups: model calls ------------------------------------------
    let cups = (snap.llm_calls / 4).min(8) as i32;
    for i in 0..cups {
        canvas.print(
            Point::new(desk_x1 - 4 - i * 2, desk_y - 1),
            "▙▟",
            mix(t.bg, ink, 0.85),
            Rgba::TRANSPARENT,
        );
    }

    // ---- the bin: failures ----------------------------------------------
    let bin_x = s.x + SCENE_W - 8;
    canvas.print(
        Point::new(bin_x, floor - 2),
        "┌───┐",
        faint,
        Rgba::TRANSPARENT,
    );
    canvas.print(
        Point::new(bin_x, floor - 1),
        "│   │",
        faint,
        Rgba::TRANSPARENT,
    );
    canvas.print(Point::new(bin_x, floor), "└───┘", faint, Rgba::TRANSPARENT);
    let misses = snap.tool_failures.min(14) as i32;
    for i in 0..misses {
        // Deterministic scatter: the same count always draws the same
        // floor, so the room is stable frame to frame.
        let (dx, dy) = match i % 6 {
            0 => (1, -1),
            1 => (2, -1),
            2 => (3, -1),
            3 => (-2, 0),
            4 => (-4, 0),
            _ => (6, 0),
        };
        let x = bin_x + 1 + dx + (i / 6);
        canvas.put(
            Point::new(x, floor + dy.max(-1)),
            'o',
            if dy < 0 {
                mix(t.bg, t.error, 0.85)
            } else {
                mix(t.bg, t.error, 0.6)
            },
            Rgba::TRANSPARENT,
        );
    }
    if misses > 0 {
        canvas.print(
            Point::new(bin_x - 1, floor + 1),
            &format!("{} failed", snap.tool_failures),
            t.error,
            Rgba::TRANSPARENT,
        );
    }
}

/// Small panes get the intern alone: the figure and the clock, no props.
fn compact(canvas: &mut dyn StyledCanvas, rect: Rect, t: &TokenSet, snap: &Snapshot, frame: u64) {
    if rect.w < 10 || rect.h < 5 {
        return;
    }
    let pose = match snap.state {
        State::Waiting => Pose::Signing,
        State::Idle | State::Down => Pose::Sleeping,
        State::Failing => Pose::Facepalm,
        _ => Pose::Typing,
    };
    let tick = snap.state.motion() > 0.0 && (frame / 4).is_multiple_of(2);
    let x = rect.x + (rect.w - 7) / 2;
    let y = rect.y + (rect.h - 5) / 2;
    for (i, row) in sprite(pose, tick).iter().enumerate() {
        canvas.print(
            Point::new(x, y + i as i32),
            row,
            t.text_muted,
            Rgba::TRANSPARENT,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every sprite row is the same width — the scene centers on it, and
    /// a ragged sprite would smear the desk.
    #[test]
    fn every_pose_is_seven_cells_wide() {
        for pose in [
            Pose::Typing,
            Pose::Reading,
            Pose::Thinking,
            Pose::Signing,
            Pose::Sleeping,
            Pose::Facepalm,
        ] {
            for tick in [false, true] {
                for row in sprite(pose, tick) {
                    assert_eq!(
                        row.chars().count(),
                        7,
                        "{pose:?} tick={tick} row {row:?} is not 7 cells"
                    );
                }
            }
        }
    }

    /// The pose is DERIVED. A run that is down or idle must never be
    /// drawn working — the pet's original sin.
    #[test]
    fn a_dead_run_is_never_drawn_working() {
        let feed = Feed::new();
        let snap = |state| Snapshot {
            state,
            phase: crate::store::Phase::Running,
            elapsed_secs: 10,
            since_event_ms: 0,
            tok_per_s: None,
            ctx_frac: None,
            llm_calls: 0,
            tool_calls: 0,
            tool_failures: 0,
            activity: String::new(),
        };
        assert_eq!(pose_of(&feed, &snap(State::Down)), Pose::Sleeping);
        assert_eq!(pose_of(&feed, &snap(State::Idle)), Pose::Sleeping);
        assert_eq!(pose_of(&feed, &snap(State::Waiting)), Pose::Signing);
        assert_eq!(pose_of(&feed, &snap(State::Failing)), Pose::Facepalm);
    }
}
