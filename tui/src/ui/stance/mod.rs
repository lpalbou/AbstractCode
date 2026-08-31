//! `/stance` — the Cognitive Monitor's conduct read, in a terminal.
//!
//! A port of AbstractUIC's `AfConductGauge` v2 ("THE STANCE"): one
//! figure whose body language IS four mechanical reads of the current
//! turn. The arcs of v1 became a posture because four dials "don't look
//! super interesting" (the operator's own verdict on the web widget);
//! the reads themselves are unchanged and live in [`conduct`], a
//! faithful port of the kit's framework-free core.
//!
//! - **EFF effort** — the spine's breath vigor and the head's size:
//!   think time + output volume against this session's own median.
//! - **ACT action** — hands: one stroke fanning right per tool call
//!   (subitizable literal counts), red tip = a failed call.
//! - **ATT attention** — roots: one filament drooping left per memory
//!   recalled; buds at the base = memories formed. **Entity lane only**:
//!   agent runs carry no recall fact (see [`lane_has_recall`]), so on the
//!   agent lane this read is not shown at all rather than dashed forever.
//! - **RIG rigor** — alignment: a high verify-shaped share stands the
//!   spine straight, a low one with many acts leaves it askew; each
//!   verify-shaped call also RINGS its stroke tip, so rigor stays
//!   countable (real tool vocabularies cluster near share ≈ 1, where
//!   alignment alone draws nothing distinctive).
//!
//! Pre-attentive channels only: count, size, alignment, colour. Absent
//! fact = a dashed hint plus its reason, never a zero-faked limb; the
//! compact legend under the figure always prints all four values,
//! because a motion/alignment encoding needs a static numeric channel
//! beside it.
//!
//! ## Where it lives
//!
//! A floating panel, bottom-right, over the transcript — NOT a row in
//! the column, which would push the conversation up every time the read
//! changed size. It is a DRAW overlay layer, which routes no input at
//! all, so scrolling, selecting and clicking the words underneath work
//! exactly as they did. Two sizes: folded (`▸`, one strip, the four
//! reads as text) and unfolded (`▾`, the bordered card with the figure).
//! **A click on the panel folds/unfolds it**, and so does `Ctrl+G` or
//! `/stance`: off → folded → unfolded → off. The click is caught by the
//! ROOT tree at capture phase ([`hit`]) rather than by the layer, which
//! is what lets the panel be input-transparent everywhere else. It sits
//! at z 500 — above the transcript, below modals and toasts, because a
//! read-out must never cover a question the app is asking.
//!
//! Inside the card each read is NAMED beside the limb it reads — code,
//! value, and the word for what that limb means. A posture whose
//! vocabulary lives in a caption underneath is a picture you have to be
//! taught; this one carries its own key.
//!
//! ## Removal
//!
//! Self-contained by design. To delete the feature: drop
//! `src/ui/stance/`, `tests/stance.rs`, `examples/stance.rs`, the
//! `pub mod stance;` line in `src/ui/mod.rs`, the four `stance` blocks
//! in `ui::mod` (the two signals + ticker + `wire_overlay` in `root`,
//! the `Ctrl+G` shortcut, the capture-phase click handler, the
//! `stance_mode` parameter on `submit` / `dispatch_command`, and the
//! dispatch arm), and the `Stance` command in `src/commands.rs`. Nothing else refers to it; it owns no state in
//! `store` and no row in the layout.

pub mod conduct;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use abstracttui::app::{LayerHandle, Overlays};
use abstracttui::base::{Point, Rect, Rgba, Size};
use abstracttui::prelude::*;
use abstracttui::text;
use abstracttui::theme::derive::mix;
use abstracttui::ui::StyledCanvas;

use crate::store::Store;
use crate::transcript::{Item, ToolStatus};
use conduct::{Axis, AxisId, Baseline, Facts, ToolCall};

/// How `/stance` is showing. 0 = not at all.
pub const OFF: u8 = 0;
/// One row: the four reads as text, under the transcript.
pub const LINE: u8 = 1;
/// The figure plus its legend.
pub const FIGURE: u8 = 2;

/// Session window for the running medians, as the kit uses.
const BASELINE_WINDOW: usize = 12;
/// Caps, from the kit: strokes, filaments, buds.
const ACT_CAP: usize = 10;
const ATT_CAP: usize = 12;
const FORMED_CAP: usize = 5;
/// Spine segments.
const SEGS: i32 = 8;

// ---------------------------------------------------------------------------
// Facts from the transcript
// ---------------------------------------------------------------------------

/// One turn's mechanical facts plus its calls.
#[derive(Debug, Clone, Default)]
pub struct Turn {
    pub facts: Facts,
    pub tools: Vec<ToolCall>,
}

/// Split the fold into turns at each user message and read each one.
///
/// A turn's `tool_rounds` is the number of CONTIGUOUS blocks of tool
/// calls (a model cycle that called tools = one round), which is what
/// the kit's `tool_rounds` means. `memories_recalled` stays `None` here:
/// an agent run's ledger has no such field at all — see
/// [`lane_has_recall`] for why that is structural, not a gap.
pub fn turns(store: Store) -> Vec<Turn> {
    store.fold.with(walk)
}

/// [`turns`] without tracking — for draw closures, which must peek.
fn turns_untracked(store: Store) -> Vec<Turn> {
    store.fold.with_untracked(walk)
}

/// The one walk. Tracked and untracked callers share it, so the reads
/// can never differ between a repaint and a re-render.
fn walk(f: &crate::transcript::Fold) -> Vec<Turn> {
    {
        let mut out: Vec<Turn> = Vec::new();
        let mut cur = Turn::default();
        let mut started = false;
        let mut in_tools = false;
        for item in f.items.iter() {
            match item {
                Item::User { .. } => {
                    if started {
                        out.push(std::mem::take(&mut cur));
                    }
                    started = true;
                    in_tools = false;
                }
                Item::Thinking { call, .. } => {
                    in_tools = false;
                    if let Some(ms) = call.gen_time_ms {
                        cur.facts.think_ms = Some(cur.facts.think_ms.unwrap_or(0.0) + ms);
                    }
                    if call.output_tokens > 0 {
                        cur.facts.tokens_out =
                            Some(cur.facts.tokens_out.unwrap_or(0) + call.output_tokens);
                    }
                    if call.input_tokens > 0 {
                        cur.facts.tokens_in = Some(call.input_tokens);
                    }
                }
                Item::Tool { name, status, .. } => {
                    if !in_tools {
                        cur.facts.tool_rounds = Some(cur.facts.tool_rounds.unwrap_or(0) + 1);
                        in_tools = true;
                    }
                    cur.tools.push(ToolCall {
                        name: name.clone(),
                        ok: match status {
                            ToolStatus::Ok => Some(true),
                            ToolStatus::Failed | ToolStatus::Denied => Some(false),
                            _ => None,
                        },
                    });
                }
                _ => in_tools = false,
            }
        }
        if started {
            out.push(cur);
        }
        out
    }
}

/// Medians of the turns BEFORE the current one — the session's own
/// history, never an invented constant.
pub fn baseline(prior: &[Turn]) -> Baseline {
    let pick = |f: fn(&Turn) -> Option<f64>| -> Vec<f64> { prior.iter().filter_map(f).collect() };
    Baseline {
        think_ms: conduct::running_median(&pick(|t| t.facts.think_ms), BASELINE_WINDOW),
        tokens_out: conduct::running_median(
            &pick(|t| t.facts.tokens_out.map(|v| v as f64)),
            BASELINE_WINDOW,
        ),
        tool_rounds: conduct::running_median(
            &pick(|t| t.facts.tool_rounds.map(|v| v as f64)),
            BASELINE_WINDOW,
        ),
        memories_recalled: conduct::running_median(
            &pick(|t| t.facts.memories_recalled.map(|v| v as f64)),
            BASELINE_WINDOW,
        ),
    }
}

/// Whether the CURRENT lane can produce a memory-recall fact at all.
///
/// It cannot on the agent lane. Memory belongs to the entity visit
/// endpoint (`entities::TurnResponse::memories` / `diary_entries`); an
/// agent run's ledger carries no such field, because our agents do not
/// run on top of the memory graph — entities do. The kit's "absent fact
/// = absent reading, with its reason" rule is for a fact that SOMETIMES
/// fails to arrive; a read that can NEVER arrive is not an honest blank,
/// it is clutter that teaches the reader the instrument is broken. So on
/// the agent lane the axis is not dashed — it is not shown.
pub fn lane_has_recall(store: Store) -> bool {
    matches!(store.focus.get(), crate::convo::Focus::Entity(_))
}

/// The reads THIS LANE can actually make. [`read`] keeps returning the
/// full four — the contract is the kit's, and the entity lane has all of
/// them — so the filtering lives here, at the render boundary.
pub fn visible(store: Store, axes: Vec<Axis>) -> Vec<Axis> {
    if lane_has_recall(store) {
        return axes;
    }
    axes.into_iter()
        .filter(|a| a.id != AxisId::Attention)
        .collect()
}

/// Whether this session has a turn to read at all. Four dashes are an
/// honest answer to "how is this turn going" before anything has been
/// asked, but they are a poor one — the view says the plain thing
/// instead.
pub fn has_turn(store: Store) -> bool {
    store
        .fold
        .with(|f| f.items.iter().any(|i| matches!(i, Item::User { .. })))
}

/// The reads for the newest turn, against the ones before it.
pub fn read(store: Store) -> (Vec<Axis>, Turn) {
    let mut all = turns(store);
    let current = all.pop().unwrap_or_default();
    let base = baseline(&all);
    (
        conduct::axes(&current.facts, &current.tools, &base),
        current,
    )
}

/// [`read`] for a DRAW closure: peeks only, tracks nothing (the overlay
/// effect owns the tracking and the repaint).
pub fn read_untracked(store: Store) -> (Vec<Axis>, Turn) {
    let mut all = turns_untracked(store);
    let current = all.pop().unwrap_or_default();
    let base = baseline(&all);
    (
        conduct::axes(&current.facts, &current.tools, &base),
        current,
    )
}

fn has_turn_untracked(store: Store) -> bool {
    store
        .fold
        .with_untracked(|f| f.items.iter().any(|i| matches!(i, Item::User { .. })))
}

fn visible_untracked(store: Store, axes: Vec<Axis>) -> Vec<Axis> {
    if matches!(store.focus.get_untracked(), crate::convo::Focus::Entity(_)) {
        return axes;
    }
    axes.into_iter()
        .filter(|a| a.id != AxisId::Attention)
        .collect()
}

// ---------------------------------------------------------------------------
// Ink
// ---------------------------------------------------------------------------

/// The kit's four hues, expressed in the theme's own audited categorical
/// ramp — a terminal app renders on 26 palettes, so the identity colour
/// has to come from the theme rather than from a hex constant.
fn axis_ink(id: AxisId, t: &TokenSet) -> Rgba {
    match id {
        AxisId::Effort => t.chart[3],
        AxisId::Action => t.chart[6],
        AxisId::Attention => t.chart[1],
        AxisId::Rigor => t.chart[5],
    }
}

// ---------------------------------------------------------------------------
// The one-line read
// ---------------------------------------------------------------------------

/// `EFF 4.2s · ACT 3·1✕ · ATT — · RIG 2/3` — the compact legend, which
/// is also the whole `/stance line` view. Codes faint, values in their
/// axis ink, absent reads as a plain dash.
pub fn draw_line(canvas: &mut dyn StyledCanvas, rect: Rect, t: &TokenSet, axes: &[Axis]) {
    if rect.w < 24 || rect.h < 1 {
        return;
    }
    let mut x = rect.x + 1;
    let limit = rect.x + rect.w - 1;
    for (i, axis) in axes.iter().enumerate() {
        if i > 0 {
            if x + 3 > limit {
                return;
            }
            canvas.print(
                Point::new(x, rect.y),
                " · ",
                t.text_faint,
                Rgba::TRANSPARENT,
            );
            x += 3;
        }
        let code = axis.id.code();
        if x + code.len() as i32 + 1 + axis.short.chars().count() as i32 > limit {
            return;
        }
        canvas.print(Point::new(x, rect.y), code, t.text_faint, Rgba::TRANSPARENT);
        x += code.len() as i32 + 1;
        let ink = if axis.value.is_none() {
            t.text_faint
        } else {
            axis_ink(axis.id, t)
        };
        canvas.print(Point::new(x, rect.y), &axis.short, ink, Rgba::TRANSPARENT);
        x += axis.short.chars().count() as i32;
    }
}

// ---------------------------------------------------------------------------
// The figure
// ---------------------------------------------------------------------------

/// Stable per-index pseudo-random in -1..=1 — the kit's `hash01`, so the
/// same turn always draws the same posture (no flicker across frames).
fn hash01(k: i32) -> f32 {
    let x = (k as f32 * 127.1 + 311.7).sin() * 43758.545;
    (x - x.floor()) * 2.0 - 1.0
}

fn axis(axes: &[Axis], id: AxisId) -> Option<&Axis> {
    axes.iter().find(|a| a.id == id)
}

/// The posture. `moving` is the honesty gate: a run that is not
/// producing does not breathe.
pub fn draw_figure(
    canvas: &mut dyn StyledCanvas,
    rect: Rect,
    t: &TokenSet,
    axes: &[Axis],
    turn: &Turn,
    frame: u64,
    moving: bool,
) {
    if rect.w < 30 || rect.h < 6 {
        // Too small for a body: the legend alone is the honest fallback.
        return draw_line(canvas, rect, t, axes);
    }
    let eff = axis(axes, AxisId::Effort);
    let act = axis(axes, AxisId::Action);
    let att = axis(axes, AxisId::Attention);
    let rig = axis(axes, AxisId::Rigor);

    let breath = eff.and_then(|a| a.value).unwrap_or(0.0);
    let align = rig.and_then(|a| a.value).unwrap_or(1.0);
    // Labels, where there is room for them: the figure has to explain
    // itself. A posture whose vocabulary lives in a caption underneath is
    // a picture you have to be taught — the codes sit BESIDE the limbs
    // they read, in the limb's own ink, so the link is the layout.
    let labels = rect.w >= 30;
    let label_col = rect.x + rect.w - LABEL_W;
    let body_w = if labels { rect.w - LABEL_W } else { rect.w };
    let cx = rect.x + body_w / 2;
    let head_y = rect.y;
    let spine_top = rect.y + 2;
    // The spine takes what the box has, down to a stub: the figure is
    // drawn in a panel the operator can resize by folding, so a fixed
    // segment count would overflow it rather than sit in it.
    let segs = (rect.h - 5).clamp(3, SEGS);
    let base_y = spine_top + segs;
    let ink = t.text_muted;

    // Breath: a small idle sway so a quiet turn still reads as alive,
    // scaled up by effort — and ZERO when nothing is producing.
    let sway = if moving {
        let ph = (frame % 24) as f32 / 24.0 * std::f32::consts::TAU;
        (0.35 + 1.15 * breath) * ph.sin()
    } else {
        0.0
    };

    // ---- spine: alignment IS rigor ------------------------------------
    // The walk moves at most one cell per row and draws the connector
    // where it steps, so a low-rigor spine reads as ASKEW rather than as
    // a column of disconnected fragments (a terminal cannot slant a
    // line, so the diagonal glyph is the slant).
    let mut spine_x = [0i32; SEGS as usize];
    let mut x = cx + sway.round() as i32;
    for i in 0..segs {
        let want = cx
            + ((1.0 - align) * hash01(i) * 2.4 + sway * (1.0 - i as f32 / segs as f32)).round()
                as i32;
        let step = (want - x).clamp(-1, 1);
        let glyph = match step {
            -1 => '╱',
            1 => '╲',
            _ => '┃',
        };
        x += step;
        spine_x[i as usize] = x;
        canvas.put(Point::new(x, spine_top + i), glyph, ink, Rgba::TRANSPARENT);
    }

    // ---- head: size is a STATIC read of effort (motion alone was
    // invisible in practice, and vanishes under reduced motion) --------
    // Floor of two cells: one faint cell is not a channel, it is a speck.
    let head_w = 2 + (breath * 2.0).round() as i32;
    let head_ink = if eff.and_then(|a| a.value).is_some() {
        axis_ink(AxisId::Effort, t)
    } else {
        t.text_faint
    };
    // Anchored on the spine's own top, so the body is one object.
    let hx = spine_x[0] - (head_w - 1) / 2;
    canvas.print(
        Point::new(hx, head_y),
        &"▄".repeat(head_w.max(1) as usize),
        head_ink,
        Rgba::TRANSPARENT,
    );
    canvas.print(
        Point::new(hx, head_y + 1),
        &"▀".repeat(head_w.max(1) as usize),
        head_ink,
        Rgba::TRANSPARENT,
    );

    // ---- hands: one stroke per call, fanning right --------------------
    match act {
        Some(a) if a.value.is_none() && turn.tools.is_empty() => {
            hint(canvas, cx + 3, spine_top + 2, t, "- - -");
        }
        _ => {
            let mut drawn = 0usize;
            for (i, call) in turn.tools.iter().take(ACT_CAP).enumerate() {
                let row = spine_top + i as i32;
                if row >= base_y {
                    break;
                }
                drawn += 1;
                let sx = spine_x[(i as i32).min(segs - 1) as usize] + 1;
                let len = 3 + (i % 3) as i32;
                if sx + len + 1 >= rect.x + rect.w {
                    continue;
                }
                canvas.print(
                    Point::new(sx, row),
                    &"─".repeat(len as usize),
                    mix(t.bg, axis_ink(AxisId::Action, t), 0.55),
                    Rgba::TRANSPARENT,
                );
                // The tip carries the two facts a call has: did it fail,
                // and is its name verification-shaped.
                let failed = call.ok == Some(false);
                let verify = conduct::is_verify_shaped(&call.name);
                let (glyph, tip) = match (failed, verify) {
                    (true, _) => ('✕', t.error),
                    (false, true) => ('○', axis_ink(AxisId::Rigor, t)),
                    // `·` vanished beside `○` at this size, so a turn of
                    // mixed calls read as "all lookups" (operator, at a
                    // glance). A filled tip counts as loudly as a ringed one.
                    (false, false) => ('▪', axis_ink(AxisId::Action, t)),
                };
                canvas.put(Point::new(sx + len, row), glyph, tip, Rgba::TRANSPARENT);
            }
            // What did not FIT, not what exceeded the cap: in a folded
            // card the fan is short, and "6 calls" over five strokes has
            // to say so.
            let overflow = turn.tools.len().saturating_sub(drawn);
            if overflow > 0 {
                canvas.print(
                    Point::new(cx + 6, base_y - 1),
                    &format!("+{overflow}"),
                    t.text_faint,
                    Rgba::TRANSPARENT,
                );
            }
        }
    }

    // ---- roots: one filament per recalled memory, drooping left -------
    let recalled = turn.facts.memories_recalled.unwrap_or(0) as usize;
    // A dash is drawn for a read that is PRESENT but unreadable. An axis
    // this lane cannot make at all is absent from `axes` entirely, and
    // draws nothing — no phantom limb, and nothing to explain.
    if att.is_some_and(|a| a.value.is_none() && recalled == 0) {
        hint(canvas, cx - 8, spine_top + 2, t, "- - -");
    }
    for j in 0..recalled.min(ATT_CAP) {
        let row = spine_top + 2 + j as i32;
        if row >= base_y {
            break;
        }
        let sx = spine_x[((j as i32) + 2).min(segs - 1) as usize] - 1;
        let len = 3 + (j % 3) as i32;
        if sx - len - 1 < rect.x {
            continue;
        }
        canvas.print(
            Point::new(sx - len, row),
            &"─".repeat(len as usize),
            mix(t.bg, axis_ink(AxisId::Attention, t), 0.55),
            Rgba::TRANSPARENT,
        );
        canvas.put(
            Point::new(sx - len - 1, row),
            '◦',
            axis_ink(AxisId::Attention, t),
            Rgba::TRANSPARENT,
        );
    }

    // ---- buds at the base: memories formed ----------------------------
    let formed = turn.facts.memories_formed.unwrap_or(0) as usize;
    if formed > 0 {
        let buds = formed.min(FORMED_CAP);
        canvas.print(
            Point::new(cx - buds as i32 / 2, base_y),
            &"¤".repeat(buds),
            t.ok,
            Rgba::TRANSPARENT,
        );
    }

    // ---- the labels: each read named beside the limb it reads ---------
    if labels {
        // EFF at the head.
        put_label(canvas, label_col, head_y, t, eff, "effort");
        // ACT at the top of the stroke fan, ATT at the roots, RIG at the
        // foot of the spine — the three places the eye already is.
        put_label(canvas, label_col, spine_top, t, act, "calls");
        if att.is_some() {
            put_label(canvas, label_col, spine_top + 2, t, att, "recalled");
        }
        put_label(
            canvas,
            label_col,
            base_y.min(rect.y + rect.h - 2),
            t,
            rig,
            "verify-shaped",
        );
    }

    // ---- the one line that is left: why a read is missing --------------
    // With the limbs labeled, a legend repeating them is noise; the only
    // thing left to say is why a side is bare.
    let last = rect.y + rect.h - 1;
    if last > base_y {
        if !labels {
            draw_line(canvas, Rect::new(rect.x, last - 1, rect.w, 1), t, axes);
        }
        if let Some(a) = axes.iter().find(|a| a.reason.is_some()) {
            let why = format!(
                "{} — {}",
                a.id.label(),
                a.reason.clone().unwrap_or_default()
            );
            let why = text::truncate_ellipsis(&why, (rect.w - 2).max(4));
            canvas.print(
                Point::new(rect.x + 1, last),
                &why,
                t.text_faint,
                Rgba::TRANSPARENT,
            );
        }
    }
}

/// Cells reserved on the right for the labels.
const LABEL_W: i32 = 15;

/// One label: the axis code in its own ink, its value, and — on the row
/// below, if it fits — the word for what the limb beside it means.
fn put_label(
    canvas: &mut dyn StyledCanvas,
    x: i32,
    y: i32,
    t: &TokenSet,
    axis: Option<&Axis>,
    means: &str,
) {
    let Some(a) = axis else { return };
    canvas.print(
        Point::new(x, y),
        a.id.code(),
        axis_ink(a.id, t),
        Rgba::TRANSPARENT,
    );
    let value = text::truncate_ellipsis(&a.short, LABEL_W - 5);
    canvas.print(
        Point::new(x + 4, y),
        &value,
        if a.value.is_none() {
            t.text_faint
        } else {
            t.text
        },
        Rgba::TRANSPARENT,
    );
    let means = text::truncate_ellipsis(means, LABEL_W - 1);
    canvas.print(
        Point::new(x, y + 1),
        &means,
        t.text_faint,
        Rgba::TRANSPARENT,
    );
}

/// The dashed hint an ABSENT read draws in place of a limb.
fn hint(canvas: &mut dyn StyledCanvas, x: i32, y: i32, t: &TokenSet, s: &str) {
    canvas.print(Point::new(x, y), s, t.text_faint, Rgba::TRANSPARENT);
}

// ---------------------------------------------------------------------------
// The view + the command
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// The floating panel
// ---------------------------------------------------------------------------

/// Panel geometry: the folded strip and the unfolded card, in cells.
/// The card is deliberately small — it floats OVER the transcript, so
/// every row it takes is a row of the conversation it hides.
const CARD_W: i32 = 38;
const CARD_H: i32 = 12;
/// Clearance from the viewport's right edge.
const MARGIN_X: i32 = 2;
/// Fixed width for the folded strip, so the panel does not jitter left
/// and right as the numbers change width.
const STRIP_W: i32 = 44;
/// Z: above the transcript, below modals (1000) and toasts (2000) — a
/// read-out must never sit on top of a question the app is asking.
const PANEL_Z: i32 = 500;

/// Where the panel sits for a given mode: bottom-right, above the
/// composer, clamped into the viewport.
pub fn panel_bounds(mode: u8, view: Size) -> Rect {
    let (w, h) = match mode {
        FIGURE => (
            CARD_W.min(view.w - 2),
            CARD_H.min(view.h - crate::ui::CHROME_ROWS - 1),
        ),
        _ => (STRIP_W.min(view.w - 2), 1),
    };
    let (w, h) = (w.max(1), h.max(1));
    let x = (view.w - w - MARGIN_X).max(0);
    let y = (view.h - crate::ui::CHROME_ROWS - h).max(0);
    Rect::new(x, y, w, h)
}

/// Does this screen cell fall on the panel? The panel itself is a DRAW
/// layer and receives no input by design (so the transcript underneath
/// stays scrollable and selectable); the root tree asks THIS at capture
/// phase and consumes the press only when it lands here.
pub fn hit(mode: u8, view: Size, pos: Point) -> bool {
    if mode == OFF {
        return false;
    }
    let b = panel_bounds(mode, view);
    pos.x >= b.x && pos.x < b.x + b.w && pos.y >= b.y && pos.y < b.y + b.h
}

/// Paint the panel: an opaque card on `surface_raised` so the words
/// underneath do not bleed through, a hairline border, and the read.
#[allow(clippy::too_many_arguments)]
fn paint_panel(
    canvas: &mut dyn StyledCanvas,
    rect: Rect,
    t: &TokenSet,
    mode: u8,
    axes: &[Axis],
    turn: &Turn,
    frame: u64,
    moving: bool,
    has_turn: bool,
) {
    let ground = t.surface_raised;
    canvas.fill(rect, ' ', t.text_muted, ground);
    if mode != FIGURE {
        // Folded: one strip, a chevron that says it opens, the reads.
        canvas.print(
            Point::new(rect.x + 1, rect.y),
            "▸",
            t.text_faint,
            Rgba::TRANSPARENT,
        );
        if has_turn {
            draw_line(
                canvas,
                Rect::new(rect.x + 2, rect.y, rect.w - 2, 1),
                t,
                axes,
            );
        } else {
            canvas.print(
                Point::new(rect.x + 3, rect.y),
                "stance — no turn yet",
                t.text_faint,
                Rgba::TRANSPARENT,
            );
        }
        return;
    }
    // Unfolded: a bordered card, title row, figure, legend.
    let b = t.border;
    let top = format!("╭─ ▾ stance {}╮", "─".repeat((rect.w - 14).max(0) as usize));
    canvas.print(Point::new(rect.x, rect.y), &top, b, Rgba::TRANSPARENT);
    let bottom = format!("╰{}╯", "─".repeat((rect.w - 2).max(0) as usize));
    canvas.print(
        Point::new(rect.x, rect.y + rect.h - 1),
        &bottom,
        b,
        Rgba::TRANSPARENT,
    );
    for y in (rect.y + 1)..(rect.y + rect.h - 1) {
        canvas.put(Point::new(rect.x, y), '│', b, Rgba::TRANSPARENT);
        canvas.put(
            Point::new(rect.x + rect.w - 1, y),
            '│',
            b,
            Rgba::TRANSPARENT,
        );
    }
    let inner = Rect::new(rect.x + 1, rect.y + 1, rect.w - 2, rect.h - 2);
    if !has_turn {
        canvas.print(
            Point::new(inner.x + 1, inner.y + inner.h / 2),
            "no turn yet — the reads appear",
            t.text_faint,
            Rgba::TRANSPARENT,
        );
        return;
    }
    draw_figure(canvas, inner, t, axes, turn, frame, moving);
}

/// Mount the panel as an overlay layer and keep it in sync.
///
/// A DRAW layer, deliberately: it paints over the transcript but routes
/// no input, so selecting and scrolling the conversation underneath keep
/// working exactly as before. Geometry changes (fold, unfold, resize)
/// recreate the layer — a layer's bounds are fixed at creation — and
/// everything else is a `damage()`.
pub fn wire_overlay(
    cx: Scope,
    store: Store,
    overlays: Overlays,
    mode: Signal<u8>,
    frame: Signal<u64>,
) {
    /// (mode, x, y, w, h) — a layer's bounds are fixed at creation, so
    /// this is what tells a repaint from a re-mount.
    type Geometry = (u8, i32, i32, i32, i32);
    let layer: Rc<RefCell<Option<LayerHandle>>> = Rc::new(RefCell::new(None));
    let geometry: Rc<Cell<Geometry>> = Rc::new(Cell::new((OFF, 0, 0, 0, 0)));
    cx.effect(move || {
        let m = mode.get();
        let view = abstracttui::app::current_viewport();
        // Tracked so the panel repaints as the run moves: the frame
        // clock, the phase, and the transcript's own length.
        let f = frame.get();
        let moving = store.phase.get() != crate::store::Phase::Idle;
        let _ = store.fold.with(|fold| fold.items.len());
        let mut slot = layer.borrow_mut();
        if m == OFF {
            if let Some(h) = slot.take() {
                h.remove();
            }
            geometry.set((OFF, 0, 0, 0, 0));
            return;
        }
        let bounds = panel_bounds(m, view);
        let want = (m, bounds.x, bounds.y, bounds.w, bounds.h);
        if let Some(h) = slot.as_ref() {
            if geometry.get() == want {
                h.damage(); // same box, new numbers
                return;
            }
        }
        if let Some(h) = slot.take() {
            h.remove();
        }
        geometry.set(want);
        *slot = Some(overlays.layer_draw(PANEL_Z, bounds, move |canvas, rect| {
            // Draw purity: peeks only, no signal tracking, no layer
            // mutation — the effect above owns both.
            let t = abstracttui::app::current_theme().tokens;
            let has = has_turn_untracked(store);
            let (axes, turn) = read_untracked(store);
            let axes = visible_untracked(store, axes);
            paint_panel(canvas, rect, &t, m, &axes, &turn, f, moving, has);
        }));
    });
}

/// `/stance [line|figure|off]`. Bare cycles off → line → figure → off.
pub fn command(mode: Signal<u8>, arg: Option<&str>) -> String {
    let now = mode.get_untracked();
    let next = match arg
        .map(str::trim)
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "" => (now + 1) % 3,
        "off" | "none" | "0" => OFF,
        "line" | "1" | "on" => LINE,
        "figure" | "stance" | "2" | "full" => FIGURE,
        other => return format!("/stance: unknown mode {other:?} — try line, figure or off"),
    };
    mode.set(next);
    match next {
        LINE => "stance: the turn's four reads — effort, action, attention, rigor".into(),
        FIGURE => {
            "stance: the figure — breath is effort, strokes are calls, alignment is rigor".into()
        }
        _ => "stance off".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `/stance` cycles off → line → figure → off, names its refusals,
    /// and a refusal changes nothing.
    #[test]
    fn the_command_cycles_and_refuses_by_name() {
        abstracttui::reactive::create_root(|cx| {
            let mode = cx.signal(OFF);
            assert_eq!(mode.get_untracked(), OFF, "off by default: opt-in");
            command(mode, None);
            assert_eq!(mode.get_untracked(), LINE);
            command(mode, None);
            assert_eq!(mode.get_untracked(), FIGURE);
            command(mode, None);
            assert_eq!(mode.get_untracked(), OFF, "the cycle closes");
            command(mode, Some("figure"));
            assert_eq!(mode.get_untracked(), FIGURE);
            let msg = command(mode, Some("sideways"));
            assert!(msg.contains("unknown mode"), "{msg}");
            assert_eq!(mode.get_untracked(), FIGURE, "a refusal changes nothing");
            command(mode, Some("off"));
            assert_eq!(mode.get_untracked(), OFF);
        });
    }

    /// The panel sits bottom-right, clear of the composer, inside the
    /// viewport — at every size, including ones too small to hold it.
    #[test]
    fn the_panel_stays_bottom_right_and_inside_the_viewport() {
        for view in [
            Size::new(200, 60),
            Size::new(110, 30),
            Size::new(80, 24),
            Size::new(40, 12),
            Size::new(20, 8),
        ] {
            for mode in [LINE, FIGURE] {
                let b = panel_bounds(mode, view);
                assert!(b.x >= 0 && b.y >= 0, "{view:?}/{mode}: {b:?}");
                assert!(
                    b.x + b.w <= view.w,
                    "{view:?}/{mode}: {b:?} runs off the right edge"
                );
                assert!(
                    b.y + b.h <= view.h - crate::ui::CHROME_ROWS,
                    "{view:?}/{mode}: {b:?} covers the composer"
                );
            }
        }
        // Unfolded is taller than folded, and folded is exactly one row.
        let view = Size::new(110, 30);
        assert_eq!(panel_bounds(LINE, view).h, 1);
        assert!(panel_bounds(FIGURE, view).h > 1);
    }
}
