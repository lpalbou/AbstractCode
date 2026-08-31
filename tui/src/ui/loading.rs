//! The session-loading screen: what the pane shows while a `/sessions`
//! pick (or a boot `--resume`) rehydrates history from the gateway.
//!
//! Before this existed the restore window — up to ~21 HTTP bundle
//! fetches, seconds on tool-heavy sessions — rendered the SPLASH, whose
//! "describe a task below" guidance is a lie about a session with
//! history in flight; the only truth was one faint strip line. Now the
//! pane itself is the waiting surface (operator ask, 2026-08-28):
//! the brand lockup with its sheen, a spinner, and a progress bar that
//! turns determinate the moment the run list lands.
//!
//! Rules inherited from the rest of the ambient surfaces:
//!
//! - **Never fabricate.** The bar is a function of counters the worker
//!   actually posted (`store.restore_progress`): it SWEEPS while the
//!   denominator is unknown (the run-list fetch) and FILLS per fetched
//!   turn bundle after — a fake percentage would be the strip's old
//!   "no runs yet" lie with better production values.
//! - **Zero-wakeup discipline.** No ticker of its own: the frame clock
//!   is the splash ticker, whose arming predicate includes `restoring`
//!   (one predicate in `ui::root` owns the wakeup budget).
//! - **Honest degradation.** Same shape as the idle screen: every row
//!   `shrink(0.0)`, the column clips, short panes drop bottom rows
//!   whole (captions first, the lockup last).

use abstracttui::prelude::*;
use abstracttui::text;
use abstracttui::widgets::{Progress, Spinner, SpinnerKind};

use crate::store::Store;
use crate::ui::logo;

/// The bar's design width in cells; narrow panes flex-shrink it.
const BAR_W: i32 = 44;

/// Cells the indeterminate band advances per ticker frame (~150 ms):
/// one full pass over the design-width bar every ~3.6 s — visibly a
/// scanner, never a filling bar pretending to know the total.
const SCAN_STEP: u64 = 3;

/// The words under the bar, from the worker's own counters. `None` =
/// the run list is still in flight; `Some((_, 0))` = nothing to replay
/// (the probe is checking for a live run before it hands off).
fn caption(progress: Option<(usize, usize)>) -> String {
    match progress {
        None => "contacting the gateway — listing this session's runs…".to_string(),
        Some((_, 0)) => "no prior turns to fetch — checking for a live run…".to_string(),
        Some((done, total)) => {
            format!("restored {done} of {total} prior turn(s) in full detail")
        }
    }
}

/// The loading pane. Reads `session_id` and `restore_progress` (tracked
/// — the caller's dyn re-renders as the worker posts counters); `frame`
/// is the splash ticker's clock, already read at the branch site.
pub fn view(t: &TokenSet, store: Store, frame: u64) -> View {
    // The same entrance as the idle screen: a session switch resets the
    // ticker, so the surface fades up from the ground instead of
    // popping (`ui::logo::boot_fade`, one curve everywhere).
    let fade = logo::boot_fade(frame);
    let ground = t.bg;
    let dim = move |c: Rgba| logo::lerp_ink(ground, c, fade);
    let muted = dim(t.text_muted);
    let faint = dim(t.text_faint);
    let progress = store.restore_progress.get();
    let sid = crate::ui::chrome::tail_ellipsis(&store.session_id.get(), 28);

    let line = |s: String, ink: Rgba| {
        Element::new()
            .style(LayoutStyle::line(1).shrink(0.0))
            .draw(move |canvas, rect| {
                let fitted = text::truncate_ellipsis(&s, (rect.w - 2).max(4));
                let w = text::width(&fitted);
                let x = rect.x + ((rect.w - w) / 2).max(0);
                canvas.print(Point::new(x, rect.y), &fitted, ink, Rgba::TRANSPARENT);
            })
            .build()
    };
    // Grow-spacer: centers the block vertically (column) and each
    // widget row horizontally (rows below); collapses first on short
    // panes so the block top-aligns before anything clips.
    let flex = || {
        Element::new()
            .style(LayoutStyle::default().grow(1.0))
            .build()
    };
    // One row of the block, its content centered between two spacers.
    let centered = |content: View| {
        Element::new()
            .style(LayoutStyle::row().height(Dimension::Cells(1)).shrink(0.0))
            .child(flex())
            .child(content)
            .child(flex())
            .build()
    };

    let mut col = Element::new()
        .style(
            LayoutStyle::column()
                .grow(1.0)
                .gap(1)
                .padding(Edges::hv(0, 1))
                .clip(),
        )
        .draw(move |canvas, rect| {
            canvas.fill(rect, ' ', ground, ground);
        })
        .child(flex());

    // The identity lockup leads, exactly as on the idle screen — the
    // loading surface is the app waiting, not a different app.
    let tagline = format!("{} · rendered by AbstractTUI", crate::cli::VERSION);
    col = col.child(
        match logo::hero_rows(abstracttui::app::current_viewport().h) {
            Some(mark_rows) => logo::hero(t, frame, &tagline, mark_rows, fade),
            None => logo::logo(t, frame, &tagline, fade),
        },
    );

    // The spinner: the engine widget, pure over the ticker frame — the
    // one glyph that says "alive" even while the bar waits on a fetch.
    col = col.child(centered(
        Spinner::new()
            .kind(SpinnerKind::Braille)
            .frame(frame)
            .label(format!("restoring session {sid}"))
            .element(t)
            .build(),
    ));

    // The bar: determinate = the engine's Progress (eighth-block
    // sub-cell fill) over the worker's counters; indeterminate = the
    // lockup's own sheen band sweeping the same track.
    let bar = match progress {
        Some((done, total)) if total > 0 => Progress::new(done as f32 / total as f32)
            .layout(
                LayoutStyle::default()
                    .width(Dimension::Cells(BAR_W))
                    .height(Dimension::Cells(1)),
            )
            .element(t)
            .build(),
        _ => scan_bar(t, frame),
    };
    col = col.child(centered(bar));

    col = col.child(line(caption(progress), muted));
    col = col.child(line(
        "history is durable on the gateway — the conversation lands whole".into(),
        faint,
    ));
    col.child(flex()).build()
}

/// The indeterminate bar: the sheen band from the lockup's light,
/// crossing a `surface_raised` track in accent ink. Same soft cosine
/// edge, faster clock — unknown totals get honest MOTION, not a number.
fn scan_bar(t: &TokenSet, frame: u64) -> View {
    let track = t.surface_raised;
    let accent = t.accent;
    Element::new()
        .style(
            LayoutStyle::default()
                .width(Dimension::Cells(BAR_W))
                .height(Dimension::Cells(1)),
        )
        .draw(move |canvas, rect| {
            if rect.w <= 0 || rect.h <= 0 {
                return;
            }
            let pos = logo::sheen_pos(frame.wrapping_mul(SCAN_STEP), rect.w);
            for x in 0..rect.w {
                let k = logo::sheen_weight(x, 0, pos);
                let ink = logo::lerp_ink(track, accent, k);
                canvas.put(Point::new(rect.x + x, rect.y), ' ', ink, ink);
            }
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The caption never invents a total: unknown stays "listing…",
    /// zero says so, and a real denominator counts honestly.
    #[test]
    fn captions_follow_the_workers_counters() {
        assert!(caption(None).contains("listing this session's runs"));
        assert!(caption(Some((0, 0))).contains("no prior turns"));
        let c = caption(Some((3, 9)));
        assert!(c.contains("restored 3 of 9"), "{c}");
        // A failed fetch still advances `done` (the error is carded in
        // the fold): the caption never sticks below its own bar.
        assert!(caption(Some((9, 9))).contains("restored 9 of 9"));
    }

    /// The scanner MOVES, and crosses the whole bar.
    ///
    /// The first cut asserted only that the band's position wraps —
    /// which `sheen_pos`'s own modulo guarantees for EVERY integer
    /// multiplier, so it held with `SCAN_STEP` set to 1, 100 or 4097
    /// and pinned nothing about the constant it named (adversarial
    /// review 2026-08-28, H-2). What actually matters is that
    /// consecutive ticker frames differ (a bar that repeats its
    /// position reads as frozen — the honest-motion contract) and that
    /// a sweep takes a human-scaled few seconds rather than a blur.
    #[test]
    fn the_scan_band_advances_every_frame_and_sweeps_the_bar() {
        let w = BAR_W;
        // Every frame moves the band: SCAN_STEP=0 would freeze it.
        for f in 0..64u64 {
            let now = logo::sheen_pos(f.wrapping_mul(SCAN_STEP), w);
            let next = logo::sheen_pos((f + 1).wrapping_mul(SCAN_STEP), w);
            assert_ne!(now, next, "the band is static at frame {f}");
        }
        // Every column is BRIGHTLY lit at some frame of one pass: the
        // scanner crosses the whole bar rather than pacing one end.
        // The floor is 0.9, not 1.0, because the band advances in
        // SCAN_STEP-cell jumps and so does not land dead-centre on
        // every column (measured: column 0 peaks at 0.970). A column
        // the band never reaches scores 0.0, which is what this
        // catches; demanding a perfect 1.0 would only be asserting
        // that SCAN_STEP divides the period.
        let frames = ((w + 2 * 14) as u64).div_ceil(SCAN_STEP) + 1;
        for x in 0..w {
            let peak = (0..frames)
                .map(|f| logo::sheen_weight(x, 0, logo::sheen_pos(f * SCAN_STEP, w)))
                .fold(0.0f32, f32::max);
            assert!(peak > 0.9, "column {x} never lit (peak {peak})");
        }
        // A pass lands in seconds at the ~150 ms ticker, not frames.
        let secs = frames as f32 * 0.150;
        assert!(
            (2.0..8.0).contains(&secs),
            "a sweep takes {secs:.1}s — too fast to read or too slow to reassure"
        );
    }
}
