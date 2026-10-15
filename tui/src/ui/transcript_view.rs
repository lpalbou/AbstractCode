//! Transcript pane: fold items projected into a `widgets::Feed`.
//!
//! The engine owns the hard parts since 0.2.0 — keyed items, windowed
//! paint, measured content extent, follow-tail — so this module is a
//! PROJECTION: `Item` → `FeedItem` (rich span headers since 0.2.3 —
//! our 0102 filing — plus capped custom body blocks, real markdown
//! blocks for assistant bodies, mosaic cells for images), plus a sync
//! effect that keeps the feed matched to the fold.
//!
//! Sync contract (`wire_feed`): appends and in-place item updates ride
//! the keyed fast path (`push`/`update` by `i{index}` — O(changed));
//! anything that changes WHICH items are visible or how EVERYTHING
//! renders (details toggle, theme switch, session reset, an item
//! folding away) rebuilds through `FeedState::clear()` — the engine's
//! documented rebuild seam. Correctness rule behind the split: feed
//! order is PUSH order, so a key may only be appended when it lands at
//! the tail; mid-list visibility changes force the rebuild path.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use abstracttui::prelude::*;
use abstracttui::render::{RichLine, RichText, Span, Style};
use abstracttui::text;
use abstracttui::widgets::{CustomBlock, Feed, FeedBlock, FeedItem, FeedState};

use crate::convo::Focus;
use crate::store::Store;
use crate::transcript::{Item, ToolStatus};

const IMAGE_ROWS: i32 = 14;
const TOOL_RESULT_MAX_ROWS: usize = 6;

// ---------------------------------------------------------------------------
// Card rendering: rich span HEADERS (engine 0.2.3 — the span model our
// 0102 filing motivated) + capped custom BODY blocks (the row-cap /
// overflow-marker / hang-indent features rich blocks don't carry;
// filed engine-side as first-app/0283 — convert when it ships)
// ---------------------------------------------------------------------------

/// One styled header row — glyph + label + detail, each with its own
/// ink — as an engine rich line: typeset through the same span-
/// preserving wrap as every block, no custom draw closure. Empty parts
/// are omitted. HONEST DELTA vs the old custom header: a long detail
/// WRAPS at draw width instead of ellipsizing to the remaining columns
/// (rich lines have no truncate knob — first-app/0283 files it). Since
/// 2026-08-20 the details tool card carries NO args in its header —
/// they are their own uncapped block below it, because a one-line hint
/// in the full view cut the very thing the card exists to show. The
/// folded row still carries the bounded `args_preview` hint, and that
/// row is a `ToolRow` custom block, not this header.
fn header_line(
    glyph: &str,
    glyph_ink: Rgba,
    label: &str,
    label_ink: Rgba,
    detail: &str,
    detail_ink: Rgba,
) -> RichLine {
    let mut spans = Vec::new();
    if !glyph.is_empty() {
        spans.push(Span::new(format!("{glyph} "), Style::new().fg(glyph_ink)));
    }
    if !label.is_empty() {
        spans.push(Span::new(label, Style::new().fg(label_ink)));
    }
    if !detail.is_empty() {
        spans.push(Span::new(
            format!("  {detail}"),
            Style::new().fg(detail_ink),
        ));
    }
    RichLine::from_spans(spans)
}

/// A header-led feed item: a leading BLANK row baked into the rich
/// block (the feed packs at gap 0 so collapsed tool rows can group
/// under their cycle; sections carry their own air — the engine's
/// typeset rhythm blanks only BETWEEN blocks, never before the first),
/// then the header row; blocks follow.
fn carded(
    glyph: &str,
    glyph_ink: Rgba,
    label: &str,
    label_ink: Rgba,
    detail: &str,
    detail_ink: Rgba,
) -> FeedItem {
    FeedItem::new().rich_block(RichText::from_lines(vec![
        RichLine::new(),
        header_line(glyph, glyph_ink, label, label_ink, detail, detail_ink),
    ]))
}

/// Two-row section rule: one blank row, then `── label ────────…`
/// across the item width — the transcript's section delimiter
/// (operator directive 2026-08-19: "we do not see the turns
/// clearly"). TWO WEIGHTS carry the hierarchy (adversarial review
/// finding 3): '═' marks the TURN level (the user's ask, the final
/// answer), '─' the cycle level (one model call) — without the split,
/// thirty identical cycle rules drowned the five-character `❯ you`
/// that opened them. Draw-width-aware, so it needs a custom block
/// (rich lines wrap; a rule must fill exactly one row); the blank is
/// baked in because the typeset rhythm never blanks before an item's
/// first block.
///
/// Since 2026-10-10 the rules left are the final answer's, drawn by
/// [`rule_block_with_icon`] (the user's message is a card, a cycle a
/// panel row), and the run separators of a read-only transcript
/// ([`rule_block`]); this is their paint — row 1 of a 2-row block, row 0
/// the air.
fn draw_rule(
    canvas: &mut dyn abstracttui::ui::StyledCanvas,
    rect: Rect,
    ch: char,
    label: &str,
    label_ink: Rgba,
    line_ink: Rgba,
) {
    if rect.w <= 0 || rect.h < 2 {
        return;
    }
    let y = rect.y + 1;
    let lead = format!("{ch}{ch} ");
    canvas.print(Point::new(rect.x, y), &lead, line_ink, Rgba::TRANSPARENT);
    let lx = rect.x + text::width(&lead);
    let fitted = text::truncate_ellipsis(label, (rect.w - text::width(&lead) - 4).max(4));
    canvas.print(Point::new(lx, y), &fitted, label_ink, Rgba::TRANSPARENT);
    let tx = lx + text::width(&fitted) + 1;
    if rect.right() > tx {
        let tail: String = ch.to_string().repeat((rect.right() - tx) as usize);
        canvas.print(Point::new(tx, y), &tail, line_ink, Rgba::TRANSPARENT);
    }
}

/// A labeled full-width rule, two rows: air, then `ch ch label ch…`.
fn rule_block(ch: char, label: String, label_ink: Rgba, line_ink: Rgba) -> FeedBlock {
    FeedBlock::Custom(CustomBlock::new(
        |_| 2,
        move |canvas, rect| draw_rule(canvas, rect, ch, &label, label_ink, line_ink),
    ))
}

/// [`rule_block`] with the message's play/pause icon at its right end
/// (the final answer's `══ assistant ══ ▶ ═`).
fn rule_block_with_icon(
    ch: char,
    label: String,
    label_ink: Rgba,
    line_ink: Rgba,
    icon: VoiceIcon,
    icon_ink: Rgba,
    icon_lit: Rgba,
) -> FeedBlock {
    // THREE rows — air, the rule, air — because the answer's markdown is
    // its own feed entry (`render_body`): the engine sets TWO blank rows
    // between a custom block and a markdown block in one item (measured:
    // operator report 2026-10-10, "one too many line"), and none between
    // items, so the air below the rule is drawn here, exactly one row.
    FeedBlock::Custom(CustomBlock::new(
        |_| 3,
        move |canvas, rect| {
            draw_rule(canvas, rect, ch, &label, label_ink, line_ink);
            if rect.h >= 2 {
                paint_icon(
                    canvas,
                    rect,
                    rect.y + 1,
                    icon,
                    icon_ink,
                    icon_lit,
                    Rgba::TRANSPARENT,
                );
            }
        },
    ))
}

/// The collapsed tool row: ONE line that never wraps — status glyph,
/// name, `· status-word` inline (the SAME position as the full card's
/// header, so toggling /details never moves the operator's scan
/// column — adversarial review finding 4), then the faint args hint.
/// Degradation order on narrow panes: the hint shrinks first, then
/// the name ellipsizes; the tag survives (operator directive
/// 2026-08-19: the collapsed view is "just the call + a success/
/// failure/ongoing tag"). An error ATTACHES directly below the row —
/// same block, so no typeset-rhythm blank can detach it from its `✗`
/// (adversarial review round 2, F4): `↳ ` first line, hang-indented
/// continuations, capped with the honest marker.
/// THE disclosure marker for a folded tool card — `None` when the card
/// hides nothing. ONE function, called by both the draw and the click
/// hit-test, because a marker the renderer draws at a width the hit-test
/// computes differently is a control that misses its own target.
///
/// Why it exists at all (operator ask 2026-08-28, and an ADR 0001 gap
/// that predates it): the folded row shows glyph, name, status word,
/// args hint and error — and the tool's OUTPUT not at all. A call that
/// printed 214 lines rendered identically to one that printed nothing.
/// Every other lane in this transcript names what it is holding back —
/// the thinking card says `… (+441 words of reasoning · /details)`, and
/// every capped body says `… (+K more lines)` — so the tool row was the
/// one place the house rule was not applied to itself.
///
/// The count is the RESULT's line count and nothing else: that is the
/// body the folded view omits entirely, and it is exactly what
/// expanding reveals, so the number the marker promises and the rows it
/// delivers are the same number. Args and errors are already on screen
/// (previewed and one-lined, each with its own ellipsis), so folding
/// them in here would inflate the promise.
pub fn tool_marker(result: &str, status: ToolStatus, expanded: bool, width: i32) -> Option<String> {
    // A running call has produced nothing YET — a marker there would
    // claim hidden output that does not exist.
    if status == ToolStatus::Running {
        return None;
    }
    marker_text(tool_body_rows(result, width).len(), expanded)
}

/// THE body of an expanded tool card, at a width — the one place the
/// rows are computed, so the marker's number and the drawn rows cannot
/// disagree (adversarial review 2026-08-29, D2: counting
/// `result.lines()` promised `▸ 3 lines` for `"\n\n\n"` and delivered
/// ZERO rows, because wrapping pops trailing blanks; a 300-column line
/// promised 1 and delivered 5). Width-dependent by construction, which
/// is also how the engine's own `… (+K more lines)` behaves.
///
/// The `│ ` gutter marks tool OUTPUT — the same voice `/details` uses.
fn tool_body_rows(result: &str, width: i32) -> Vec<String> {
    if result.is_empty() {
        return Vec::new();
    }
    wrap_rows(result, (width - 4).max(4))
        .iter()
        .map(|l| format!("│ {l}"))
        .collect()
}

/// The tool output as the transcript shows it: JSON laid out
/// (`ui::json_view`), anything else verbatim. ONE function for the folded
/// row's body, its `▸ N lines` marker and the marker's hit-test, so the
/// three count the same rows.
pub(crate) fn shown_result(result: &str) -> std::borrow::Cow<'_, str> {
    if crate::ui::json_view::is_json(result) {
        std::borrow::Cow::Owned(crate::ui::json_view::pretty(result))
    } else {
        std::borrow::Cow::Borrowed(result)
    }
}

fn marker_text(rows: usize, expanded: bool) -> Option<String> {
    if rows == 0 {
        return None;
    }
    let arrow = if expanded { '▾' } else { '▸' };
    let unit = if rows == 1 { "line" } else { "lines" };
    Some(format!("{arrow} {rows} {unit}"))
}

/// The folded tool row's horizontal layout, in offsets from the row's
/// left edge. Computed ONCE and consumed by BOTH the draw and the click
/// hit-test, because they disagreed: the draw drops a marker it cannot
/// fit, the hit-test had no such term, and on a narrow pane a left
/// click on ordinary row text silently expanded the body and swallowed
/// the press that would have started a text selection — a control where
/// no glyph was drawn (adversarial review 2026-08-29, D1).
struct RowLayout {
    hint_x: i32,
    hint_avail: i32,
    marker_x: i32,
    /// The marker is DRAWN here. When false those cells carry nothing
    /// and are not a control.
    marker_fits: bool,
}

fn tool_row_layout(
    w: i32,
    glyph: &str,
    name: &str,
    word_seg: &str,
    marker: Option<&str>,
) -> RowLayout {
    let word_w = text::width(word_seg);
    let mut x = text::width(glyph) + 1;
    let name_avail = w - x - word_w - 1;
    if name_avail > 0 {
        x += text::width(&text::truncate_ellipsis(name, name_avail)) + 1;
    }
    x += word_w + 2;
    let marker_w = marker.map(text::width).unwrap_or(0);
    let marker_x = w - marker_w;
    let marker_fits = marker_w > 0 && marker_x > x + 1;
    let hint_avail = if marker_fits { marker_x - 1 - x } else { w - x };
    RowLayout {
        hint_x: x,
        hint_avail,
        marker_x,
        marker_fits,
    }
}

struct ToolRow {
    glyph: &'static str,
    glyph_ink: Rgba,
    name: String,
    name_ink: Rgba,
    hint: String,
    hint_ink: Rgba,
    word: &'static str,
    word_ink: Rgba,
    error: String,
    error_ink: Rgba,
    /// Workspace root for `linkify`: the hint's path/URL tokens carry
    /// OSC-8 targets (same cells, same inks — see `ui::linkify`).
    link_root: Option<Rc<str>>,
    /// The output this folded row hides — drawn inline (with the `│`
    /// gutter that marks tool OUTPUT, the same voice `/details` uses)
    /// when `expanded`, and counted by [`tool_marker`] when not.
    result: String,
    result_ink: Rgba,
    marker_ink: Rgba,
    status: ToolStatus,
    expanded: bool,
    /// Width-keyed memo of the wrapped body. The draw runs on EVERY
    /// repaint — a streaming run repaints per ledger record — and a
    /// custom block is handed its FULL rect, so re-wrapping an expanded
    /// 30k-line result per frame measured 51 ms/frame (adversarial
    /// review 2026-08-29, D6). The clip cull skips the PRINTS; only
    /// this skips the wrap.
    rows: RefCell<Option<(i32, Rc<Vec<String>>)>>,
}

impl ToolRow {
    /// The body at `width`, wrapped at most once per width.
    fn rows_at(&self, width: i32) -> Rc<Vec<String>> {
        if let Some((w, rows)) = self.rows.borrow().as_ref() {
            if *w == width {
                return rows.clone();
            }
        }
        let rows = Rc::new(tool_body_rows(&self.result, width));
        *self.rows.borrow_mut() = Some((width, rows.clone()));
        rows
    }

    fn marker(&self, width: i32) -> Option<String> {
        if self.status == ToolStatus::Running {
            return None;
        }
        marker_text(self.rows_at(width).len(), self.expanded)
    }

    /// The block's declared height. NAMED (not inlined in the
    /// `CustomBlock` closure) so a test can call the real arithmetic:
    /// the engine keeps `CustomBlock::height` private, and a test that
    /// re-adds the terms itself is a tautology — deleting the body term
    /// from the closure left the whole suite green.
    ///
    /// Must cover EVERY row the draw paints, or the block paints over
    /// the item below it.
    fn height_at(&self, width: i32) -> i32 {
        1 + self.error_lines(width).len() as i32 + self.body_lines(width).len() as i32
    }

    /// The rows expansion adds. UNCAPPED on purpose: expanding is a
    /// deliberate act on ONE card, so it obeys the same rule
    /// `/details` does — "details means details" (operator directive
    /// 2026-08-20). The marker counts exactly these rows.
    fn body_lines(&self, width: i32) -> Rc<Vec<String>> {
        if !self.expanded {
            return Rc::new(Vec::new());
        }
        self.rows_at(width)
    }

    fn error_lines(&self, width: i32) -> Vec<String> {
        if self.error.is_empty() {
            return Vec::new();
        }
        // Indent 2 + "↳ " prefix: the error voice, never the model's.
        let (lines, _) = wrap_capped(&self.error, width - 4, 3);
        lines
            .iter()
            .enumerate()
            .map(|(i, l)| {
                if i == 0 {
                    format!("↳ {l}")
                } else {
                    format!("  {l}")
                }
            })
            .collect()
    }

    fn block(self) -> FeedBlock {
        let spec = Rc::new(self);
        let h_spec = spec.clone();
        FeedBlock::Custom(CustomBlock::new(
            // Delegates, so the height a test can call IS the height
            // the engine gets. Inlining the arithmetic here left
            // `height_at` orphaned and its test a tautology.
            move |width| h_spec.height_at(width),
            move |canvas, rect| {
                if rect.w <= 0 || rect.h <= 0 {
                    return;
                }
                let word_seg = format!("· {}", spec.word);
                let mut x = rect.x;
                canvas.print(
                    Point::new(x, rect.y),
                    spec.glyph,
                    spec.glyph_ink,
                    Rgba::TRANSPARENT,
                );
                x += text::width(spec.glyph) + 1;
                // Name budget keeps the tag alive: everything up to the
                // right edge minus the status word and its separator. On
                // a pane too narrow for any name, the tag still prints
                // (the operator's directive is the tag, clipped if must).
                let name_avail = rect.right() - x - text::width(&word_seg) - 1;
                if name_avail > 0 {
                    let fitted_name = text::truncate_ellipsis(&spec.name, name_avail);
                    canvas.print(
                        Point::new(x, rect.y),
                        &fitted_name,
                        spec.name_ink,
                        Rgba::TRANSPARENT,
                    );
                    x += text::width(&fitted_name) + 1;
                }
                canvas.print(
                    Point::new(x, rect.y),
                    &word_seg,
                    spec.word_ink,
                    Rgba::TRANSPARENT,
                );
                // The disclosure marker is RIGHT-ALIGNED and outranks
                // the args hint for space. The hint is a preview that
                // already ellipsizes when squeezed; the marker is the
                // ADR-0001 statement that a whole body is off screen,
                // and it is bounded where the hint is elastic. On a pane
                // too narrow for both, the marker survives and the hint
                // shrinks — and where even the marker cannot fit it is
                // DROPPED WHOLE (honest degradation), which is exactly
                // the case the hit-test must agree about, so BOTH read
                // the same `tool_row_layout`.
                let marker = spec.marker(rect.w);
                let lay =
                    tool_row_layout(rect.w, spec.glyph, &spec.name, &word_seg, marker.as_deref());
                let hx = rect.x + lay.hint_x;
                if lay.hint_avail >= 4 && !spec.hint.is_empty() {
                    let fitted = text::truncate_ellipsis(&spec.hint, lay.hint_avail);
                    // Same cells and ink as a plain print; path/URL
                    // tokens additionally carry OSC-8 targets. The
                    // FITTED string is what segments, so a token the
                    // ellipsis cut simply is not a token any more.
                    crate::ui::linkify::print_linked(
                        canvas,
                        Point::new(hx, rect.y),
                        &fitted,
                        spec.hint_ink,
                        spec.link_root.as_ref(),
                    );
                }
                if lay.marker_fits {
                    if let Some(m) = &marker {
                        canvas.print(
                            Point::new(rect.x + lay.marker_x, rect.y),
                            m,
                            spec.marker_ink,
                            Rgba::TRANSPARENT,
                        );
                    }
                }
                let errors = spec.error_lines(rect.w);
                for (i, line) in errors.iter().enumerate() {
                    canvas.print(
                        Point::new(rect.x + 2, rect.y + 1 + i as i32),
                        line,
                        spec.error_ink,
                        Rgba::TRANSPARENT,
                    );
                }
                // The expanded body, below the error (the same order
                // `/details` renders: what failed, then what it said).
                // Off-screen rows are culled here for the same reason
                // `CappedBody` culls them: a custom block is handed its
                // FULL rect and an expanded 3000-line result would
                // otherwise re-wrap and print every invisible row on
                // every repaint.
                let clip = canvas.clip_rect();
                let body_top = rect.y + 1 + errors.len() as i32;
                for (i, line) in spec.body_lines(rect.w).iter().enumerate() {
                    let y = body_top + i as i32;
                    if y < clip.y || y >= clip.bottom() {
                        continue;
                    }
                    canvas.print(
                        Point::new(rect.x + 2, y),
                        line,
                        spec.result_ink,
                        Rgba::TRANSPARENT,
                    );
                }
            },
        ))
    }
}

/// A capped, wrapped, one-ink body block: wraps at draw width, caps the
/// row count with the honest "… (+K more lines)" marker, optional
/// hang-indented line prefix (the `· ` of info items). The height
/// callback and the draw share `lines_at`, so the geometry is honest by
/// construction. Deliberately still a custom block: rich feed blocks
/// (0.2.3) wrap span-true but carry NO width-aware row cap, overflow
/// marker, or hanging indent — the exact features these previews exist
/// for (filed as first-app/0283).
/// The command sandbox of one tool call (R12.1 / R14.4): "Sandbox: macOS
/// sandbox-exec · 3 workspaces enforced" (or "Sandbox: none — refused"),
/// then — when `detail` — each enforced path with its mode. Nothing for a
/// tool whose result carries no sandbox evidence.
fn sandbox_blocks(
    t: &TokenSet,
    sandbox: Option<&crate::sandbox_line::ToolSandbox>,
    detail: bool,
) -> Vec<FeedBlock> {
    let Some(sb) = sandbox else {
        return Vec::new();
    };
    let ink = match sb.state {
        crate::sandbox_line::SandboxState::Sandboxed => t.text_muted,
        crate::sandbox_line::SandboxState::Refused => t.warn,
        crate::sandbox_line::SandboxState::Unsandboxed => t.error,
    };
    let mut out = vec![CappedBody::new(&sb.line, ink, 2)
        .prefix("↳ ")
        .uncapped(true)
        .block()];
    let rows = sb.detail_lines();
    if detail && !rows.is_empty() {
        out.push(
            CappedBody::new(&rows.join("\n"), t.text_faint, rows.len().max(1))
                .prefix("  ")
                .uncapped(true)
                .block(),
        );
    }
    out
}

/// The repaint key of a tool call's sandbox (its line and paths).
fn sandbox_key(sandbox: Option<&crate::sandbox_line::ToolSandbox>) -> String {
    sandbox.map_or(String::new(), |sb| {
        let mut k = sb.line.clone();
        for l in sb.detail_lines() {
            k.push('\n');
            k.push_str(&l);
        }
        k
    })
}

struct CappedBody {
    body: String,
    ink: Rgba,
    cap: usize,
    indent: i32,
    /// Prefix the first line; continuations hang-indent under it.
    prefix: String,
    /// One blank row ABOVE the body — the section break for items
    /// whose first block this is (the typeset rhythm blanks only
    /// between blocks, never before an item's first).
    lead: bool,
    /// `│ ` gutter on EVERY line (adversarial review finding 10):
    /// result bodies wear it so a scrolled-into screenful of output is
    /// identifiable as TOOL OUTPUT — bare indent stays the model's
    /// voice, `↳` the error's. Mutually exclusive with `prefix` by
    /// usage.
    bar: bool,
    /// Tail-preserving elision (`wrap_capped_tail`) — result bodies,
    /// where the last lines are the punchline.
    keep_tail: bool,
    /// `/details` (operator directive 2026-08-20: "when we show ALL the
    /// details, we do NOT truncate anything"). Every row is rendered —
    /// no cap, no `… (+N more lines)`. The fold's caps stay for the
    /// FOLDED view, which is a summary by definition.
    uncapped: bool,
    /// This body's path/URL tokens carry OSC-8 links (`ui::linkify` —
    /// tool ARGS and RESULTS only; prose bodies stay plain prints).
    /// `wrap_rows` breaks at words, so tokens survive wrapping whole
    /// and classify on their own line.
    link: bool,
    /// Workspace root for relative-path resolution; absolute paths and
    /// URLs link without one.
    link_root: Option<Rc<str>>,
}

impl CappedBody {
    fn new(body: &str, ink: Rgba, cap: usize) -> CappedBody {
        CappedBody {
            body: body.to_string(),
            ink,
            cap,
            indent: 2,
            prefix: String::new(),
            lead: false,
            bar: false,
            keep_tail: false,
            uncapped: false,
            link: false,
            link_root: None,
        }
    }

    /// Hyperlink this body's path/URL tokens (tool args/results).
    fn linked(mut self, root: Option<&Rc<str>>) -> CappedBody {
        self.link = true;
        self.link_root = root.cloned();
        self
    }

    /// Render every row: `/details` truncates nothing.
    fn uncapped(mut self, on: bool) -> CappedBody {
        self.uncapped = on;
        self
    }

    fn prefix(mut self, prefix: &str) -> CappedBody {
        self.prefix = prefix.into();
        self
    }

    fn no_indent(mut self) -> CappedBody {
        self.indent = 0;
        self
    }

    fn lead(mut self) -> CappedBody {
        self.lead = true;
        self
    }

    fn bar(mut self) -> CappedBody {
        self.bar = true;
        self
    }

    fn keep_tail(mut self) -> CappedBody {
        self.keep_tail = true;
        self
    }

    fn capped(&self, width: i32) -> Vec<String> {
        if self.uncapped {
            return wrap_rows(&self.body, width);
        }
        if self.keep_tail {
            wrap_capped_tail(&self.body, width, self.cap)
        } else {
            wrap_capped(&self.body, width, self.cap).0
        }
    }

    fn lines_at(&self, width: i32) -> Vec<String> {
        if self.body.is_empty() || (self.cap == 0 && !self.uncapped) {
            return Vec::new();
        }
        if self.bar {
            // Gutter on every line, wrap narrowed to make room.
            let lines = self.capped(width - self.indent - 2);
            return lines.iter().map(|l| format!("│ {l}")).collect();
        }
        let lines = self.capped(width - self.indent);
        if self.prefix.is_empty() {
            lines
        } else {
            // Prefix the FIRST line only; continuations hang-indent under
            // it. Per-line bullets made one wrapped notice read as
            // several separate ones (live review, 2026-07-22).
            let hang = " ".repeat(text::width(&self.prefix).max(0) as usize);
            lines
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    if i == 0 {
                        format!("{}{l}", self.prefix)
                    } else {
                        format!("{hang}{l}")
                    }
                })
                .collect()
        }
    }

    fn block(self) -> FeedBlock {
        let spec = Rc::new(self);
        let h_spec = spec.clone();
        FeedBlock::Custom(CustomBlock::new(
            move |width| {
                let mut h = h_spec.lines_at(width).len() as i32;
                if h_spec.lead {
                    h += 1;
                }
                h
            },
            move |canvas, rect| {
                let top = rect.y + i32::from(spec.lead);
                let lines = spec.lines_at(rect.w);
                // A custom block gets its FULL rect (its top may sit
                // above the visible band) and the canvas clips. Cull
                // off-screen rows OURSELVES (adversarial review
                // 2026-08-28, B): an uncapped `/details` result body is
                // thousands of lines of which ~20 are on screen, and
                // `linkify` stats every relative path token it is
                // handed — measured at 10.35 ms per repaint on a
                // 3000-line, path-dense body, against a repaint per
                // streamed ledger record. Clipped, the cost tracks the
                // viewport instead of the body.
                let clip = canvas.clip_rect();
                for (i, line) in lines.iter().enumerate() {
                    let y = top + i as i32;
                    if y < clip.y || y >= clip.bottom() {
                        continue;
                    }
                    let p = Point::new(rect.x + spec.indent, y);
                    if spec.link {
                        crate::ui::linkify::print_linked(
                            canvas,
                            p,
                            line,
                            spec.ink,
                            spec.link_root.as_ref(),
                        );
                    } else {
                        canvas.print(p, line, spec.ink, Rgba::TRANSPARENT);
                    }
                }
            },
        ))
    }
}

/// Cap slack (adversarial review round 2, F2): a marker that hides at
/// most this many rows costs more than it saves — one row of "…
/// (+3 more lines)" to suppress three. Show the rows instead; the cap
/// only bites when it genuinely earns its row.
const CAP_SLACK: usize = 3;

fn wrap_rows(source: &str, width: i32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in source.lines() {
        if raw.trim().is_empty() {
            lines.push(String::new());
            continue;
        }
        lines.extend(text::wrap(raw, width.max(4)));
    }
    while lines.last().map(|l| l.is_empty()).unwrap_or(false) {
        lines.pop();
    }
    lines
}

fn wrap_capped(source: &str, width: i32, cap: usize) -> (Vec<String>, usize) {
    let mut lines = wrap_rows(source, width);
    let total = lines.len();
    if total > cap.saturating_add(CAP_SLACK) {
        lines.truncate(cap);
        let hidden = total - cap;
        // The count is the honest fact; no pointer at server internals
        // (operator ruling 2026-07-26). Where a fuller client view
        // exists, a nearby marker already names it (the collapsed
        // thinking card's "/details" note).
        lines.push(format!("… (+{hidden} more lines)"));
    }
    let n = lines.len();
    (lines, n)
}

/// Tail-preserving cap for RESULT bodies (adversarial review round 2,
/// F2): for command output, test runs, and log tails the punchline is
/// the LAST lines (`wc`'s total, the verdict line) — head truncation
/// systematically hid the most informative rows. Head, marker, then
/// the final two rows; same honest marker grammar.
fn wrap_capped_tail(source: &str, width: i32, cap: usize) -> Vec<String> {
    let lines = wrap_rows(source, width);
    let total = lines.len();
    if total <= cap.saturating_add(CAP_SLACK) {
        return lines;
    }
    let head = cap.saturating_sub(3);
    let tail = 2usize;
    let hidden = total - head - tail;
    let mut out: Vec<String> = lines[..head].to_vec();
    out.push(format!("… (+{hidden} more lines)"));
    out.extend(lines[total - tail..].iter().cloned());
    out
}

fn tool_glyph(t: &TokenSet, status: ToolStatus) -> (&'static str, Rgba) {
    (tool_glyph_str(status), tool_glyph_ink(t, status))
}

/// The glyph alone — the click hit-test needs the row's geometry and
/// has no theme in hand.
fn tool_glyph_str(status: ToolStatus) -> &'static str {
    match status {
        ToolStatus::AwaitingApproval => "?",
        ToolStatus::Running => "»",
        ToolStatus::Ok => "✓",
        ToolStatus::Failed => "✗",
        ToolStatus::Denied => "⊘",
        ToolStatus::Interrupted => "◌",
    }
}

fn tool_glyph_ink(t: &TokenSet, status: ToolStatus) -> Rgba {
    match status {
        ToolStatus::AwaitingApproval => t.warn,
        ToolStatus::Running => t.accent,
        ToolStatus::Ok => t.ok,
        ToolStatus::Failed => t.error,
        ToolStatus::Denied | ToolStatus::Interrupted => t.text_muted,
    }
}

/// The explicit status word every tool row carries (operator directive
/// 2026-08-19: a glyph alone is not a "success/failure/ongoing tag").
fn tool_status_word(status: ToolStatus) -> &'static str {
    match status {
        ToolStatus::AwaitingApproval => "awaiting approval",
        ToolStatus::Running => "running",
        ToolStatus::Ok => "ok",
        ToolStatus::Failed => "failed",
        ToolStatus::Denied => "denied",
        ToolStatus::Interrupted => "interrupted",
    }
}

/// Whole-call duration for the cycle rule: sub-second keeps a decimal
/// (the shared humanizer would say "0s"); at and above one second the
/// ONE elapsed humanizer (`convo::fmt_elapsed`) formats it, so the
/// rule and the activity strip can never disagree on the same fact.
fn fmt_call_secs(ms: f64) -> String {
    if ms < 950.0 {
        format!("{:.1}s", ms / 1000.0)
    } else {
        crate::convo::fmt_elapsed((ms / 1000.0).round() as u64)
    }
}

// ---------------------------------------------------------------------------
// Play / pause per message (operator ask 2026-10-10: "for any message, we
// should be able to play/pause the reading voice — icons on the top right
// of a message")
// ---------------------------------------------------------------------------

/// The read-aloud state of ONE message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VoiceIcon {
    /// Not being read: `▶` reads it.
    Idle,
    /// Being read (or preparing): `‖` pauses it.
    Playing,
    /// Paused mid-read: `▶` resumes it.
    Paused,
}

/// The text a message's icon reads: your messages and the assistant's
/// final answers. `None` for every other item (no icon).
pub(crate) fn speakable(item: &Item) -> Option<&str> {
    match item {
        Item::User { text }
        | Item::Assistant {
            text,
            final_answer: true,
        } if !text.trim().is_empty() => Some(text),
        _ => None,
    }
}

/// The icon state of `item` from the voice signals (untracked reads: the
/// sync effect subscribes to them once per pass).
pub(crate) fn voice_icon(store: &Store, item: &Item) -> VoiceIcon {
    let Some(text) = speakable(item) else {
        return VoiceIcon::Idle;
    };
    if store.voice.speaking_text.get_untracked() != Some(crate::voice::text_key(text)) {
        return VoiceIcon::Idle;
    }
    match store.voice.speaking.get_untracked() {
        crate::voice::Speaking::Playing | crate::voice::Speaking::Preparing => VoiceIcon::Playing,
        crate::voice::Speaking::Paused => VoiceIcon::Paused,
        crate::voice::Speaking::Idle => VoiceIcon::Idle,
    }
}

/// Columns the icon zone takes at the right edge of a message's top row:
/// ` ▶ ` plus one cell of the row's own edge.
const ICON_ZONE: i32 = 4;

/// The icon's glyph and whether it is lit (this message is being read).
fn icon_glyph(icon: VoiceIcon) -> (&'static str, bool) {
    match icon {
        VoiceIcon::Idle => ("▶", false),
        VoiceIcon::Playing => ("‖", true),
        VoiceIcon::Paused => ("▶", true),
    }
}

/// Paint the icon zone (` ▶ `) on row `y` at the right edge of `rect`,
/// over `ground` — the SAME cells [`speak_icon_at`] treats as the control.
fn paint_icon(
    canvas: &mut dyn abstracttui::ui::StyledCanvas,
    rect: Rect,
    y: i32,
    icon: VoiceIcon,
    ink: Rgba,
    lit: Rgba,
    ground: Rgba,
) {
    if rect.w < ICON_ZONE + 8 {
        return;
    }
    let (glyph, on) = icon_glyph(icon);
    let x = rect.right() - ICON_ZONE;
    canvas.print(Point::new(x, y), " ", ink, ground);
    canvas.print(
        Point::new(x + 1, y),
        glyph,
        if on { lit } else { ink },
        ground,
    );
    canvas.print(Point::new(x + 2, y), " ", ink, ground);
}

// ---------------------------------------------------------------------------
// The user card (operator ask 2026-10-10: "a card with a distinct
// background color so it's easy to spot when we scroll")
// ---------------------------------------------------------------------------

/// The user card's ground: three quarters of the way from the page
/// background to the theme's RAISED surface. A designed token, so it is
/// calm in every theme — tinting toward the accent read as an error
/// banner where the accent is red (abstract-dark; adversarial review
/// 2026-10-10) — and distinct enough to find while scrolling; the accent
/// bar on the left edge carries the colour.
pub(crate) fn user_card_bg(t: &TokenSet) -> Rgba {
    t.bg.lerp(t.surface_raised, 0.75)
}

/// The user's message as a tinted card: one blank row of air, then a pad
/// row, the text (`❯ ` on the first line, continuations hang-indented),
/// and a closing pad row — all on [`user_card_bg`], with an accent bar
/// down the left edge. Replaces the `══ you ═══` rule, which looked like
/// every other section delimiter once scrolled.
struct UserCard {
    body: String,
    cap: usize,
    uncapped: bool,
    icon: VoiceIcon,
    icon_ink: Rgba,
    icon_lit: Rgba,
    bg: Rgba,
    bar_ink: Rgba,
    glyph_ink: Rgba,
    text_ink: Rgba,
    faint_ink: Rgba,
}

impl UserCard {
    /// Columns before the text: bar, space, `❯`, space.
    const LEFT: i32 = 4;

    fn lines_at(&self, width: i32) -> Vec<String> {
        let w = (width - Self::LEFT - 1).max(4);
        if self.uncapped {
            wrap_rows(&self.body, w)
        } else {
            wrap_capped(&self.body, w, self.cap).0
        }
    }

    /// Lead blank + top pad + text rows + bottom pad. Named so a test can
    /// call the arithmetic the engine is handed.
    fn height_at(&self, width: i32) -> i32 {
        3 + self.lines_at(width).len().max(1) as i32
    }

    fn block(self) -> FeedBlock {
        let spec = Rc::new(self);
        let h_spec = spec.clone();
        FeedBlock::Custom(CustomBlock::new(
            move |width| h_spec.height_at(width),
            move |canvas, rect| {
                if rect.w <= 0 || rect.h < 3 {
                    return;
                }
                let lines = spec.lines_at(rect.w);
                let rows = lines.len().max(1) as i32;
                let top = rect.y + 1;
                let card = Rect::new(rect.x, top, rect.w, rows + 2);
                canvas.fill(card, ' ', spec.text_ink, spec.bg);
                for y in top..top + rows + 2 {
                    canvas.print(Point::new(rect.x, y), "▌", spec.bar_ink, spec.bg);
                }
                // Top-right of the card: its play/pause icon.
                paint_icon(
                    canvas,
                    rect,
                    top,
                    spec.icon,
                    spec.icon_ink,
                    spec.icon_lit,
                    spec.bg,
                );
                let clip = canvas.clip_rect();
                for (i, line) in lines.iter().enumerate() {
                    let y = top + 1 + i as i32;
                    if y < clip.y || y >= clip.bottom() {
                        continue;
                    }
                    if i == 0 {
                        canvas.print(Point::new(rect.x + 2, y), "❯", spec.glyph_ink, spec.bg);
                    }
                    // The cap's own marker line reads in the faint voice.
                    let ink = if !spec.uncapped && line.starts_with("… (+") {
                        spec.faint_ink
                    } else {
                        spec.text_ink
                    };
                    canvas.print(Point::new(rect.x + UserCard::LEFT, y), line, ink, spec.bg);
                }
            },
        ))
    }
}

// ---------------------------------------------------------------------------
// Cycle panels (operator ask 2026-10-10: "the cycle to be a collapsible
// panel, folded by default, and only showing the first few words + key
// stats. when unfolded, we should see ALL the texts & tools")
// ---------------------------------------------------------------------------

/// Where an item sits relative to the reasoning cycles around it —
/// computed per sync pass by [`cycle_slots`] and handed to
/// `render_item`, `is_visible` and `fingerprint` alike, so the three can
/// never disagree about what a folded cycle hides.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Slot {
    /// Not inside a cycle panel: renders on its own, as before.
    Plain,
    /// A cycle's head (its `Thinking` item) and the calls it owns, in
    /// order — the folded row summarises them.
    Head {
        open: bool,
        tools: Vec<(ToolStatus, String)>,
        /// A FOLDED head right after an open panel takes one blank row
        /// of air, so it does not sit flush under the panel's last
        /// output line. Consecutive folded heads pack (one row each).
        lead: bool,
        /// The open-state key a click stores (base + first tool-call key
        /// when the cycle has a call) and the base key (see
        /// [`cycle_base_key`]). Identity only: no render reads them.
        key: String,
        base: String,
    },
    /// A tool call owned by the nearest cycle head above it.
    Member { open: bool },
}

/// The base identity of a cycle for [`Store::expanded_cycles`]: its
/// TURN (the hash of the user message that opened it), its ordinal among
/// that turn's cycles, its number, and its words. A `Thinking` card
/// carries no ledger key, and the feed's `i{index}` keys are positions a
/// truncation drain shifts, so identity is derived. Cycle numbers restart
/// every run and a tool-only cycle has no words, so number + words alone
/// collided across turns (adversarial review 2026-10-10: one click on
/// turn 2's `cycle 1` opened turn 1's as well) — hence the turn and the
/// ordinal. [`cycle_slots`] adds the cycle's first tool-call key, which
/// carries the run id, when the cycle has one.
fn cycle_base_key(turn_user: &str, ordinal: usize, item: &Item) -> Option<String> {
    let Item::Thinking {
        iteration,
        content,
        reasoning,
        ..
    } = item
    else {
        return None;
    };
    let mut h = Fnv::new();
    h.body(turn_user);
    h.u64(ordinal as u64);
    h.u64(*iteration as u64);
    h.body(content);
    h.body(reasoning);
    Some(format!("cycle:{:016x}", h.finish()))
}

/// The open-state key of every cycle head in `items` (`None` for every
/// other item) — what a click stores in [`Store::expanded_cycles`].
pub fn cycle_keys(items: &[Item]) -> Vec<Option<String>> {
    cycle_slots(items, &[], false)
        .into_iter()
        .map(|s| match s {
            Slot::Head { key, .. } => Some(key),
            _ => None,
        })
        .collect()
}

/// Assign every item its [`Slot`]. Ownership is positional — the ledger
/// folds a cycle's `Thinking` card before the calls it requested — and
/// a turn boundary (the user's message, a steer, the final answer) ends
/// the open cycle, so a later stray call never folds into an earlier
/// turn's panel. `details` (`/details full`) opens every cycle.
///
/// A cycle is open when `open_keys` holds its full key (base + first
/// tool-call key) OR its base key: a cycle opened before its first call
/// landed was stored under the base key and must stay open when the
/// call arrives.
pub(crate) fn cycle_slots(items: &[Item], open_keys: &[String], details: bool) -> Vec<Slot> {
    struct Acc {
        idx: usize,
        base: String,
        first_tool: Option<String>,
        tools: Vec<(ToolStatus, String)>,
    }
    let n = items.len();
    let mut owner: Vec<Option<usize>> = vec![None; n];
    let mut head_of: Vec<Option<usize>> = vec![None; n];
    let mut accs: Vec<Acc> = Vec::new();
    let mut cur: Option<usize> = None;
    let mut turn_user = "";
    let mut ordinal = 0usize;
    for (i, item) in items.iter().enumerate() {
        match item {
            Item::Thinking { .. } => {
                ordinal += 1;
                let base = cycle_base_key(turn_user, ordinal, item).unwrap_or_default();
                head_of[i] = Some(accs.len());
                cur = Some(accs.len());
                accs.push(Acc {
                    idx: i,
                    base,
                    first_tool: None,
                    tools: Vec::new(),
                });
            }
            Item::Tool {
                key, status, name, ..
            } => {
                if let Some(a) = cur {
                    let acc = &mut accs[a];
                    if acc.first_tool.is_none() {
                        acc.first_tool = Some(key.clone());
                    }
                    acc.tools.push((*status, name.clone()));
                    owner[i] = Some(a);
                }
            }
            Item::User { text } => {
                cur = None;
                turn_user = text;
                ordinal = 0;
            }
            Item::Steer { .. }
            | Item::Assistant {
                final_answer: true, ..
            } => cur = None,
            _ => {}
        }
    }
    let opens: Vec<(String, bool)> = accs
        .iter()
        .map(|a| {
            let key = match &a.first_tool {
                Some(t) => format!("{}|{t}", a.base),
                None => a.base.clone(),
            };
            let open = details || open_keys.contains(&key) || open_keys.contains(&a.base);
            (key, open)
        })
        .collect();
    // Second pass, in order: the lead blank depends on whether the
    // previous item belongs to an OPEN panel (its head or a shown member).
    let mut slots = Vec::with_capacity(n);
    let mut after_open = false;
    for i in 0..n {
        if let Some(k) = head_of[i] {
            let (key, open) = &opens[k];
            slots.push(Slot::Head {
                open: *open,
                tools: std::mem::take(&mut accs[k].tools),
                lead: !*open && after_open,
                key: key.clone(),
                base: accs[k].base.clone(),
            });
            after_open = *open;
        } else if let Some(k) = owner[i] {
            let open = opens[k].1;
            slots.push(Slot::Member { open });
            // A hidden member keeps whatever came before it.
            if open {
                after_open = true;
            }
        } else {
            slots.push(Slot::Plain);
            after_open = false;
        }
    }
    debug_assert!(accs.iter().all(|a| head_of[a.idx].is_some()));
    slots
}

/// The per-call facts the cycle row carries, most important first (the
/// folded row drops them from the END when squeezed).
fn cycle_stats(call: &crate::transcript::CallCost) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(ms) = call.gen_time_ms {
        out.push(fmt_call_secs(ms));
    }
    if call.input_tokens > 0 || call.output_tokens > 0 {
        out.push(format!(
            "{}↑ {}↓ tk",
            crate::ui::chrome::fmt_tokens(call.input_tokens),
            crate::ui::chrome::fmt_tokens(call.output_tokens)
        ));
    }
    // Cache reuse, PROVIDER-REPORTED only (operator ask 2026-08-19):
    // unreported (0) renders nothing — never the prev-input derivation
    // dressed up as a measurement (/cache owns the labeled estimate).
    if call.cached_tokens > 0 && call.input_tokens > 0 {
        let pct = (call.cached_tokens * 100 / call.input_tokens).min(100);
        out.push(format!("{pct}% cached"));
    }
    out
}

/// The tool summary chips: `N tools` then one glyph+count per status
/// present, in a fixed order, each in its status ink — a failure or a
/// pending approval stays visible on the folded row.
fn tool_chips(t: &TokenSet, tools: &[(ToolStatus, String)]) -> Vec<(String, Rgba)> {
    if tools.is_empty() {
        return Vec::new();
    }
    let unit = if tools.len() == 1 { "tool" } else { "tools" };
    let mut out = vec![(format!("{} {unit}", tools.len()), t.text_faint)];
    for status in [
        ToolStatus::AwaitingApproval,
        ToolStatus::Running,
        ToolStatus::Failed,
        ToolStatus::Denied,
        ToolStatus::Interrupted,
        ToolStatus::Ok,
    ] {
        let n = tools.iter().filter(|(s, _)| *s == status).count();
        if n > 0 {
            out.push((
                format!("{}{n}", tool_glyph_str(status)),
                tool_glyph_ink(t, status),
            ));
        }
    }
    out
}

/// The cycle's first words for the folded row: its content, else its
/// reasoning, on one line (the draw ellipsizes to the room it has).
///
/// The emphasis and code markers (`**`, `` ` ``) are dropped: the row is
/// plain text, so `**Cargo.toml**` read as markup noise (adversarial
/// review 2026-10-10); the open panel renders the same words as
/// markdown. A cycle with no words at all (a tool-only call) leads with
/// the names of the calls it made instead of an empty row.
fn cycle_gist(content: &str, reasoning: &str, tools: &[(ToolStatus, String)]) -> String {
    let src = if content.trim().is_empty() {
        reasoning
    } else {
        content
    };
    if src.trim().is_empty() {
        let names: Vec<&str> = tools.iter().map(|(_, n)| n.as_str()).collect();
        return crate::transcript::one_line(&names.join(", "), 400);
    }
    let plain = src.trim().replace("**", "").replace('`', "");
    crate::transcript::one_line(&plain, 400)
}

/// The cycle's header row. Folded: ONE row, no lead blank, so a run of
/// thirty folded cycles stacks as thirty scannable rows —
/// `▸ cycle 3  first words…   2 tools ✓2 · 4.1s · 9.8k↑ 210↓ tk`.
/// Open: a blank row of air, then `▾ cycle 3 ──────── chips · stats` as a
/// section rule. The row is the panel's toggle (`toggle_cycle_at`).
struct CycleHeader {
    open: bool,
    /// One blank row above a FOLDED header (see `Slot::Head::lead`).
    lead: bool,
    /// One blank row BELOW an open header whose words follow as their own
    /// feed entry (`render_body`) — the air the engine would otherwise
    /// double (see `rule_block_with_icon`).
    trail: bool,
    label: String,
    gist: String,
    /// The hidden-reasoning note (`∴ N words`), first in the right group
    /// and never dropped.
    note: Option<String>,
    chips: Vec<(String, Rgba)>,
    stats: Vec<String>,
    arrow_ink: Rgba,
    label_ink: Rgba,
    gist_ink: Rgba,
    stat_ink: Rgba,
    rule_ink: Rgba,
}

impl CycleHeader {
    /// The row (within the item) that carries the header — and the
    /// toggle. Open panels and lead-blank folded rows put it on row 1.
    fn header_row(open: bool, lead: bool) -> i32 {
        i32::from(open || lead)
    }

    /// The right-hand group with the first `n` stats kept, as painted
    /// segments: (text, ink — `None` = separator ink). ` · ` separates
    /// the note, the chip run and each stat; chips are space-joined.
    ///
    /// ORDER = priority: the tool chips first (an approval or a failure
    /// must survive any width — adversarial review 2026-10-10 found `?1`
    /// clipped at 40 columns behind the note), then the hidden-reasoning
    /// note, then the stats. Squeezing drops from the END.
    fn right_segments(&self, n: usize, note: bool) -> Vec<(String, Option<Rgba>)> {
        let mut out: Vec<(String, Option<Rgba>)> = Vec::new();
        let group = |out: &mut Vec<(String, Option<Rgba>)>| {
            if !out.is_empty() {
                out.push((" · ".into(), None));
            }
        };
        if !self.chips.is_empty() {
            group(&mut out);
            for (i, (c, ink)) in self.chips.iter().enumerate() {
                if i > 0 {
                    out.push((" ".into(), None));
                }
                out.push((c.clone(), Some(*ink)));
            }
        }
        if let Some(text) = self.note.as_ref().filter(|_| note) {
            group(&mut out);
            out.push((text.clone(), Some(self.stat_ink)));
        }
        for st in self.stats.iter().take(n) {
            group(&mut out);
            out.push((st.clone(), Some(self.stat_ink)));
        }
        out
    }

    fn right_width(&self, n: usize, note: bool) -> i32 {
        self.right_segments(n, note)
            .iter()
            .map(|(s, _)| text::width(s))
            .sum()
    }

    fn block(self) -> FeedBlock {
        let spec = Rc::new(self);
        let row = CycleHeader::header_row(spec.open, spec.lead);
        let trail = i32::from(spec.trail);
        FeedBlock::Custom(CustomBlock::new(
            move |_| 1 + row + trail,
            move |canvas, rect| {
                if rect.w <= 0 || rect.h <= 0 {
                    return;
                }
                let y = rect.y + row;
                let arrow = if spec.open { "▾ " } else { "▸ " };
                let mut x = rect.x;
                x += canvas.print(Point::new(x, y), arrow, spec.arrow_ink, Rgba::TRANSPARENT);
                x += canvas.print(
                    Point::new(x, y),
                    &spec.label,
                    spec.label_ink,
                    Rgba::TRANSPARENT,
                );
                // Room left of the right group: the folded row keeps at
                // least 12 columns of the model's words before it gives
                // up stats (from the END); chips always stay.
                let min_mid = if spec.open { 4 } else { 14 };
                let mut keep = spec.stats.len();
                let mut note = spec.note.is_some();
                while rect.right() - (x + 1) - spec.right_width(keep, note) - 1 < min_mid {
                    if keep > 0 {
                        keep -= 1;
                    } else if note {
                        note = false;
                    } else {
                        break;
                    }
                }
                let rw = spec.right_width(keep, note);
                let rx = (rect.right() - rw).max(x + 1);
                // Middle: the gist (folded) or the rule (open).
                let mid_x = x + 2;
                let mid_w = rx - 2 - mid_x;
                if spec.open {
                    let start = x + 1;
                    let n = (rx - 1 - start).max(0);
                    if n > 0 {
                        let rule: String = "─".repeat(n as usize);
                        canvas.print(
                            Point::new(start, y),
                            &rule,
                            spec.rule_ink,
                            Rgba::TRANSPARENT,
                        );
                    }
                } else if mid_w >= 4 && !spec.gist.is_empty() {
                    let fitted = text::truncate_ellipsis(&spec.gist, mid_w);
                    canvas.print(
                        Point::new(mid_x, y),
                        &fitted,
                        spec.gist_ink,
                        Rgba::TRANSPARENT,
                    );
                }
                // Right: the note, the chips (status inks), the stats.
                let mut cx = rx;
                for (seg, ink) in spec.right_segments(keep, note) {
                    let ink = ink.unwrap_or(spec.rule_ink);
                    cx += canvas.print(Point::new(cx, y), &seg, ink, Rgba::TRANSPARENT);
                }
            },
        ))
    }
}

/// Mosaic image block: bitmap → colored half/quadrant cells, aspect-
/// corrected for the ~1:2 terminal cell. Protocol-grade (kitty/iTerm2)
/// images cannot ride Feed custom blocks yet (engine backlog 0280);
/// the mosaic ladder is the honest in-feed rendering until then.
fn image_block(bitmap: Arc<abstracttui::widgets::Bitmap>) -> FeedBlock {
    FeedBlock::Custom(CustomBlock::new(
        |_| IMAGE_ROWS,
        move |canvas, rect| {
            let (bw, bh) = (bitmap.width().max(1) as f64, bitmap.height().max(1) as f64);
            // Contain-fit in screen units: a cell is ~1 wide x 2 tall,
            // so displayed aspect = cols / (2*rows).
            let aspect = bw / bh;
            let mut rows = IMAGE_ROWS.min(rect.h.max(1));
            let mut cols = ((2.0 * aspect * rows as f64).round() as i32).max(1);
            if cols > rect.w.max(1) {
                cols = rect.w.max(1);
                rows = ((cols as f64 / (2.0 * aspect)).round() as i32).clamp(1, rows);
            }
            // LIVE capabilities (0.2.2, our 0295 ask): the driver-published
            // probe-upgraded view — no more fabricated `unicode_ok/truecolor`.
            // Read at DRAW time so a mid-session probe upgrade re-renders
            // with the truth (an ASCII-only or 256-color terminal gets the
            // ladder's honest degradation instead of garbage cells).
            let caps = abstracttui::app::current_caps();
            let target = Rect::new(rect.x, rect.y, cols, rows);
            for patch in abstracttui::gfx::mosaic::render_to_cells(&bitmap, target, &caps) {
                canvas.put(patch.pos, patch.ch, patch.fg, patch.bg);
            }
        },
    ))
}

// ---------------------------------------------------------------------------
// Item -> FeedItem projection
// ---------------------------------------------------------------------------

/// Render one fold item. `None` = hidden in the current view (the
/// clean answers-only mode folds thinking + finished-OK tool cards).
/// `expanded` = this ONE item's per-card override (the disclosure
/// marker was activated). Meaningful only for a folded tool card;
/// `/details` outranks it, so under full detail everything is expanded
/// and no marker renders. `slot` = the item's place in the cycle panels
/// ([`cycle_slots`]): a folded cycle hides its calls, an open one shows
/// them as full cards.
fn render_item(
    t: &TokenSet,
    item: &Item,
    store: Store,
    details: bool,
    link_root: Option<&Rc<str>>,
    expanded: bool,
    slot: &Slot,
) -> Option<FeedItem> {
    match item {
        // The user's ask opens the turn as a tinted CARD (operator ask
        // 2026-10-10) — the one item findable at a glance while
        // scrolling, which a '═' rule shared with the answer was not.
        Item::User { text: body } => Some(
            FeedItem::new().block(
                UserCard {
                    body: body.clone(),
                    cap: 200,
                    uncapped: details,
                    icon: voice_icon(&store, item),
                    icon_ink: t.text_faint,
                    icon_lit: t.accent,
                    bg: user_card_bg(t),
                    bar_ink: t.accent,
                    glyph_ink: t.accent,
                    text_ink: t.text,
                    faint_ink: t.text_muted,
                }
                .block(),
            ),
        ),
        Item::Steer { text: body } => Some(
            carded("↪", t.warn, "steer", t.warn, "", t.warn).block(
                CappedBody::new(body, t.text_muted, 40)
                    .uncapped(details)
                    .block(),
            ),
        ),
        Item::Thinking {
            iteration,
            content,
            reasoning,
            call,
        } => {
            // The cycle header IS the turn delimiter (operator
            // directive 2026-08-19: "we do not see the turns clearly;
            // the first thing we should see in the turn is the actual
            // thinking"): a full-width rule names the cycle (+ its
            // per-call cost when the record reported one), then the
            // model's OWN WORDS lead — always visible, never behind a
            // toggle. /details gates VERBOSITY only: collapsed = a
            // capped gist; full = content AND the reasoning channel as
            // a labeled block (never coalesced — the old render dropped
            // reasoning whenever content existed).
            // A COLLAPSIBLE PANEL (operator ask 2026-10-10), folded by
            // default: one row of the model's first words + the call's
            // facts + its tool summary. Open (a click on the row, or
            // `/details full` for every cycle): the content as markdown,
            // the reasoning channel under its own label, and — as their
            // own feed items below — every call in full.
            let (open, tools, lead): (bool, &[(ToolStatus, String)], bool) = match slot {
                Slot::Head {
                    open, tools, lead, ..
                } => (*open, tools.as_slice(), *lead),
                _ => (details, &[], false),
            };
            // A folded row that shows CONTENT hides the reasoning channel:
            // it is NAMED with its size (house rule: a hidden channel is
            // named, never dropped). When the content is empty the gist
            // IS the reasoning, so nothing more is hidden.
            let note = (!open && !content.trim().is_empty() && !reasoning.trim().is_empty())
                .then(|| format!("∴ {} words", reasoning.split_whitespace().count()));
            let header = CycleHeader {
                open,
                lead,
                trail: open && (!content.trim().is_empty() || !reasoning.trim().is_empty()),
                label: format!("cycle {iteration}"),
                gist: cycle_gist(content, reasoning, tools),
                note,
                chips: tool_chips(t, tools),
                stats: cycle_stats(call),
                arrow_ink: t.accent,
                label_ink: t.text,
                gist_ink: t.text_muted,
                stat_ink: t.text_muted,
                rule_ink: t.text_faint,
            };
            // The panel's words (content + labelled reasoning) are the
            // item's BODY entry (`render_body`) when it is open.
            let fi = FeedItem::new().block(header.block());
            Some(fi)
        }
        Item::Tool {
            name,
            args_preview,
            args_full,
            status,
            result,
            error,
            sandbox,
            ..
        } => {
            // A CALLED TOOL IS ALWAYS SHOWN (operator ruling 2026-07-26)
            // and ALWAYS TAGGED (operator directive 2026-08-19): the
            // collapsed view is one packed row per call — glyph + name +
            // a faint args hint with the status word right-aligned, so a
            // cycle's calls stack under its thinking as one scannable
            // group; /details is the full card (span-wrapped args, the
            // result body). Errors render in BOTH views (honesty over
            // tidiness).
            // Inside a FOLDED cycle the call is summarised on the cycle's
            // row (its status chip) and renders nothing of its own; inside
            // an OPEN cycle it renders as the full card, whatever the
            // `/details` mode — "when unfolded, we should see ALL the
            // texts & tools" (operator ask 2026-10-10).
            let full = match slot {
                Slot::Member { open: false } => return None,
                Slot::Member { open: true } => true,
                _ => details,
            };
            let (glyph, ink) = tool_glyph(t, *status);
            let word = tool_status_word(*status);
            // Tool rows are SUBORDINATE to the cycle rule (operator
            // report 2026-08-19: bright names + a doubled green tag
            // out-shouted the delimiters): the name runs muted, and a
            // routine "ok" keeps only its green glyph — the WORD takes
            // status ink solely for exceptional states, so failures
            // and stalls still pop out of a wall of successes.
            let word_ink = if *status == ToolStatus::Ok {
                t.text_muted
            } else {
                ink
            };
            if !full {
                // The error rides INSIDE the row block (`↳` voice,
                // no rhythm blank between a `✗` and its reason).
                return Some(
                    FeedItem::new().block(
                        ToolRow {
                            glyph,
                            glyph_ink: ink,
                            name: name.clone(),
                            name_ink: t.text_muted,
                            hint: args_preview.clone(),
                            hint_ink: t.text_faint,
                            word,
                            word_ink,
                            // The fold keeps the error WHOLE now; the
                            // folded row is one line by definition, so it
                            // flattens here — `/details` shows all of it.
                            error: crate::transcript::one_line(error, 200),
                            error_ink: t.error,
                            link_root: link_root.cloned(),
                            result: shown_result(result).into_owned(),
                            result_ink: t.text_faint,
                            marker_ink: t.text_faint,
                            status: *status,
                            expanded,
                            rows: RefCell::new(None),
                        }
                        .block(),
                    ),
                )
                .map(|mut fi| {
                    // Process-spawning tools: the sandbox it ran under, one
                    // line per command (R14.4); the enforced paths when the
                    // row is opened.
                    for b in sandbox_blocks(t, sandbox.as_ref(), expanded) {
                        fi = fi.block(b);
                    }
                    fi
                });
            }
            // The full card: the tool's NAME in the info ink so a call
            // stands out of the prose and output around it; arguments in
            // the muted voice, output in the faint `│` voice below them.
            let spans = vec![
                Span::new(format!("{glyph} "), Style::new().fg(ink)),
                Span::new(name.clone(), Style::new().fg(t.info)),
                Span::new(format!(" · {word}"), Style::new().fg(word_ink)),
            ];
            let mut fi = FeedItem::new().rich_block(RichText::from_lines(vec![
                RichLine::new(),
                RichLine::from_spans(spans),
            ]));
            // The arguments WHOLE, on their own rows (2026-08-20): the
            // header's one-line hint is a folded-view device — in details
            // it cut the very thing the card exists to show (the command,
            // the patch, the path). `args_full` keeps the humane ordering
            // and cuts nothing; the header carries identity only.
            //
            // Fallback to the hint when a card carries no full copy (a
            // fold built by a path that predates the field): details must
            // never show LESS than the folded row.
            let args_body = if args_full.is_empty() {
                args_preview
            } else {
                args_full
            };
            if !args_body.is_empty() {
                fi = fi.block(
                    CappedBody::new(args_body, t.text_muted, TOOL_RESULT_MAX_ROWS)
                        .uncapped(true)
                        .linked(link_root)
                        .block(),
                );
            }
            for b in sandbox_blocks(t, sandbox.as_ref(), true) {
                fi = fi.block(b);
            }
            if !error.is_empty() {
                fi = fi.block(
                    CappedBody::new(error, t.error, 3)
                        .prefix("↳ ")
                        .uncapped(true)
                        .block(),
                );
            }
            // BOTH, when both exist (adversarial review 2026-08-20, F3):
            // this was an `else if`, so a failed tool's OUTPUT vanished —
            // silently, in every view. The output of a failed build is
            // precisely what the operator opens details for; the error
            // says THAT it failed, the body says WHY.
            if !result.is_empty()
                && *status != ToolStatus::Running
                && crate::ui::json_view::is_json(result)
            {
                // JSON output, laid out in the syntax inks (operator ask
                // 2026-10-10): indented, long strings wrapping inside the
                // transcript width instead of running off it.
                fi = fi.block(crate::ui::json_view::block(result, t));
            } else if !result.is_empty() && *status != ToolStatus::Running {
                // `│` gutter: a scrolled-into screenful of output is
                // identifiable as TOOL OUTPUT without its header
                // (adversarial review finding 10).
                fi = fi.block(
                    CappedBody::new(result, t.text_faint, TOOL_RESULT_MAX_ROWS)
                        .bar()
                        .keep_tail()
                        .uncapped(true)
                        .linked(link_root)
                        .block(),
                );
            }
            Some(fi)
        }
        Item::Assistant {
            text: body,
            final_answer,
        } => {
            // The FINAL answer closes the turn — the same '═' weight
            // that opened it (adversarial review finding 3). Interim
            // updates stay minor cards.
            if *final_answer {
                return Some(FeedItem::new().block(rule_block_with_icon(
                    '═',
                    "assistant".into(),
                    t.ok,
                    t.text_faint,
                    voice_icon(&store, item),
                    t.text_muted,
                    t.accent,
                )));
            }
            let ink = t.text_muted;
            Some(
                carded("✦", ink, "assistant (update)", ink, "", ink)
                    .block(FeedBlock::Markdown(body.clone())),
            )
        }
        Item::Image {
            artifact_id, label, ..
        } => {
            let mut fi = carded("▦", t.accent, label, t.text_muted, "", t.text_muted);
            match store.image_for(artifact_id) {
                Some(entry) => {
                    if let Some(bitmap) = entry.bitmap.clone() {
                        fi = fi.block(image_block(bitmap));
                    } else {
                        let msg = if entry.error.is_empty() {
                            "image unavailable".to_string()
                        } else {
                            entry.error.clone()
                        };
                        fi = fi.block(CappedBody::new(&msg, t.error, 2).uncapped(details).block());
                    }
                }
                None => {
                    fi = fi.block(CappedBody::new("fetching image…", t.text_faint, 1).block());
                }
            }
            Some(fi)
        }
        Item::Info { text: body } => Some(
            FeedItem::new().block(
                CappedBody::new(body, t.text_faint, 6)
                    .prefix("· ")
                    .no_indent()
                    .lead()
                    .uncapped(details)
                    .block(),
            ),
        ),
        Item::Error { text: body } => Some(
            carded("✗", t.error, "error", t.error, "", t.error)
                .block(CappedBody::new(body, t.error, 12).uncapped(details).block()),
        ),
        // Entity probe bodies (memory digests behind the always-visible
        // count chip): details-gated exactly like Thinking.
        Item::Probe { title, body } => {
            if !details {
                return None;
            }
            Some(
                carded("◈", t.text_faint, title, t.text_faint, "", t.text_faint).block(
                    CappedBody::new(body, t.text_faint, 14)
                        .uncapped(true)
                        .block(),
                ),
            )
        }
    }
}

/// The second feed entry of an item whose markdown follows a drawn header
/// — the final answer under its rule, an open cycle's words under its
/// header. Split because the engine sets two blank rows between a custom
/// block and a markdown block inside ONE item (operator report
/// 2026-10-10: "one too many line between ==assistant== and its answer")
/// and none between items; the header draws the one row of air itself.
/// Keyed `i{index}~body` right after the item's own key.
fn render_body(t: &TokenSet, item: &Item, slot: &Slot) -> Option<FeedItem> {
    match item {
        Item::Assistant {
            text,
            final_answer: true,
        } if !text.trim().is_empty() => {
            Some(FeedItem::new().block(FeedBlock::Markdown(text.clone())))
        }
        Item::Thinking {
            content, reasoning, ..
        } if matches!(slot, Slot::Head { open: true, .. }) => {
            let mut fi = FeedItem::new();
            let mut any = false;
            if !content.trim().is_empty() {
                fi = fi.block(FeedBlock::Markdown(content.trim().to_string()));
                any = true;
            }
            if !reasoning.trim().is_empty() {
                // The reasoning channel, NAMED and never coalesced into
                // the content. Label and body in ONE block, so no blank
                // row splits the label from what it labels; its own
                // blank row separates it from the content above.
                let mut lines = Vec::new();
                if any {
                    lines.push(RichLine::new());
                }
                lines.push(RichLine::from_spans(vec![
                    Span::new("∴ reasoning", Style::new().fg(t.accent_alt)),
                    Span::new(
                        format!("  {} words", reasoning.split_whitespace().count()),
                        Style::new().fg(t.text_faint),
                    ),
                ]));
                for line in reasoning.trim().lines() {
                    lines.push(RichLine::from_spans(vec![Span::new(
                        line.to_string(),
                        Style::new().fg(t.text_muted),
                    )]));
                }
                fi = fi.rich_block(RichText::from_lines(lines));
                any = true;
            }
            any.then_some(fi)
        }
        _ => None,
    }
}

/// Mirror of [`render_body`]'s `is_some()` without building anything —
/// the sync's "does this item own a body entry" bookkeeping.
fn has_body(item: &Item, slot: &Slot) -> bool {
    match item {
        Item::Assistant {
            text,
            final_answer: true,
        } => !text.trim().is_empty(),
        Item::Thinking {
            content, reasoning, ..
        } => {
            matches!(slot, Slot::Head { open: true, .. })
                && (!content.trim().is_empty() || !reasoning.trim().is_empty())
        }
        _ => false,
    }
}

/// The fold index a feed key names: `i{index}` (the item) or
/// `i{index}~body` (its body entry, [`render_body`]).
pub(crate) fn item_index(key: &str) -> Option<usize> {
    let rest = key.strip_prefix('i')?;
    rest.split('~').next()?.parse().ok()
}

/// Cheap content fingerprint (FNV-1a) over everything `render_item`
/// reads, so the sync effect re-renders exactly the items that changed.
/// `expanded` rides the fingerprint so a disclosure toggle repaints
/// exactly the card that changed. It changes an item's HEIGHT, never
/// its visibility, so the sync stays on the keyed fast path — the
/// `clear()` rebuild is reserved for visibility flips, where feed order
/// (push order) would otherwise strand a mid-list key.
fn fingerprint(item: &Item, store: &Store, expanded: bool, slot: &Slot) -> u64 {
    let mut h = Fnv::new();
    // The message's play/pause icon (a voice state change repaints only
    // the message being read).
    h.byte(voice_icon(store, item) as u8);
    // The cycle panel state: a head renders its open state and its calls'
    // status chips; a member renders (or hides) by its head's state.
    match slot {
        Slot::Plain => h.byte(0),
        Slot::Head {
            open, tools, lead, ..
        } => {
            h.byte(1);
            h.byte(u8::from(*open));
            h.byte(u8::from(*lead));
            h.u64(tools.len() as u64);
            for (status, name) in tools {
                h.byte(*status as u8);
                h.str(name);
            }
        }
        Slot::Member { open } => {
            h.byte(2);
            h.byte(u8::from(*open));
        }
    }
    match item {
        Item::User { text } => {
            h.byte(1);
            h.body(text);
        }
        Item::Steer { text } => {
            h.byte(2);
            h.body(text);
        }
        Item::Thinking {
            iteration,
            content,
            reasoning,
            call,
        } => {
            h.byte(3);
            h.u64(*iteration as u64);
            h.body(content);
            h.body(reasoning);
            // The cycle rule renders the call cost — a late usage fold
            // into an existing card must repaint it.
            h.u64(call.gen_time_ms.map(|ms| ms.to_bits()).unwrap_or(0));
            h.u64(call.input_tokens);
            h.u64(call.output_tokens);
            h.u64(call.cached_tokens);
        }
        Item::Tool {
            name,
            args_preview,
            args_full,
            status,
            result,
            error,
            sandbox,
            ..
        } => {
            h.byte(4);
            h.str(name);
            h.str(args_preview);
            h.body(args_full);
            h.byte(*status as u8);
            h.body(result);
            h.body(error);
            h.body(&sandbox_key(sandbox.as_ref()));
            h.byte(u8::from(expanded));
        }
        Item::Assistant { text, final_answer } => {
            h.byte(5);
            h.byte(u8::from(*final_answer));
            h.body(text);
        }
        Item::Image {
            artifact_id, label, ..
        } => {
            h.byte(6);
            h.str(artifact_id);
            h.str(label);
            // Image loads change the render without touching the item.
            match store.image_for(artifact_id) {
                Some(e) => {
                    h.byte(if e.bitmap.is_some() { 1 } else { 2 });
                    h.str(&e.error);
                }
                None => h.byte(0),
            }
        }
        Item::Info { text } => {
            h.byte(7);
            h.body(text);
        }
        Item::Error { text } => {
            h.byte(8);
            h.body(text);
        }
        Item::Probe { title, body } => {
            h.byte(9);
            h.str(title);
            h.body(body);
        }
    }
    h.finish()
}

struct Fnv(u64);

impl Fnv {
    fn new() -> Fnv {
        Fnv(0xcbf29ce484222325)
    }
    fn byte(&mut self, b: u8) {
        self.0 ^= b as u64;
        self.0 = self.0.wrapping_mul(0x100000001b3);
    }
    /// Whole-string hash — for the SHORT fields (names, keys, hints).
    fn str(&mut self, s: &str) {
        for b in s.as_bytes() {
            self.byte(*b);
        }
        self.byte(0xff); // field separator
    }

    /// Body hash: length + both ends, never the whole string.
    ///
    /// The fold now holds bodies WHOLE (2026-08-20), and this sweep runs
    /// over EVERY item on EVERY fold update — one per streamed ledger
    /// record. Hashing megabytes per record stalled the feed
    /// (measured by the adversarial review: ~13 ms/record in release
    /// with 40 × 155 KB results, F6). Bodies here are append-only
    /// streams or wholesale replacements, so length plus 256 bytes of
    /// each end separates every change the UI can actually receive; the
    /// undetectable case is an in-place middle edit that preserves the
    /// exact byte length, which no producer of these fields performs.
    /// Short bodies keep the exact whole-string hash.
    fn body(&mut self, s: &str) {
        const ENDS: usize = 256;
        let bytes = s.as_bytes();
        self.u64(bytes.len() as u64);
        if bytes.len() <= ENDS * 2 {
            for b in bytes {
                self.byte(*b);
            }
        } else {
            for b in &bytes[..ENDS] {
                self.byte(*b);
            }
            for b in &bytes[bytes.len() - ENDS..] {
                self.byte(*b);
            }
        }
        self.byte(0xff); // field separator
    }
    fn u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.byte(b);
        }
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Fold -> Feed sync
// ---------------------------------------------------------------------------

/// Keep `feed` matched to the fold. Fast path: keyed append/update.
/// Rebuild path (engine `clear()` seam): details toggle, theme switch,
/// fold reset, or a mid-list visibility flip (feed order is push
/// order — a key may only be APPENDED at the tail).
///
/// Truncation drains and the len-shrink trigger: a drain observed as a
/// shrink rebuilds here; a drain that REFILLS to >= the seen length
/// within one observed batch stays on the fast path, where every
/// shifted index re-renders IN PLACE under its positional key — keys
/// are positions, not item identities, so in-place replacement in
/// existing key order is exactly fold order. Same O(window) cost as a
/// rebuild, correct either way (test-pinned:
/// `truncation_drains_keep_the_feed_in_sync_with_fold_order`).
/// `link_root`: the workspace root `ui::linkify` resolves relative
/// paths against (None = only absolute paths and URLs link).
pub fn wire_feed(cx: Scope, store: Store, feed: &FeedState, link_root: Option<Rc<str>>) {
    struct SyncState {
        /// (fingerprint, visible) per source item index.
        seen: Vec<(u64, bool)>,
        /// Whether each item owns a body entry (`i{index}~body`). A flip
        /// of an EXISTING item rebuilds (a body key can only be appended
        /// at the tail, like any key).
        bodies: Vec<bool>,
        details: bool,
        theme: &'static str,
        /// WHICH conversation the seen bookkeeping describes. A focus
        /// mismatch is a full rebuild — without this dimension, a switch
        /// to a conversation with ≥ as many items would ride the keyed
        /// fast path over CROSS-CONTAMINATED bookkeeping (same-index
        /// equal-fingerprint items skipped; stale cards of the other
        /// conversation left on screen).
        focus: Focus,
    }
    let state = Rc::new(RefCell::new(SyncState {
        seen: Vec::new(),
        bodies: Vec::new(),
        details: true,
        theme: "",
        focus: Focus::Agent,
    }));
    let feed = feed.clone();
    cx.effect(move || {
        let theme = abstracttui::app::use_theme(cx).get();
        let t = theme.tokens;
        let theme_id = theme.id;
        let details = store.show_details.get();
        // Focus FIRST: a mismatch rebuilds exactly like a theme change
        // (the engine's documented clear() rebuild seam).
        let focus = store.focus.get();
        // Per-card disclosure, read ONCE per pass (a tracked read, so a
        // toggle re-runs this effect) rather than per item — the set
        // holds a handful of keys and cloning it per card would be the
        // most expensive thing in the sync.
        let expanded_keys = store.expanded_tools.get();
        // Open cycle panels — one tracked read per pass, like the tools.
        let open_cycles = store.expanded_cycles.get();
        // The play/pause icons follow the voice state (read per item
        // untracked in `voice_icon`; subscribed here once).
        let _ = store.voice.speaking.get();
        let _ = store.voice.speaking_text.get();
        // `/details` outranks the per-card override: under full detail
        // every card already shows its body, so nothing is "expanded"
        // and no marker renders.
        let is_expanded = |item: &Item| -> bool {
            !details
                && matches!(item, Item::Tool { key, .. } if expanded_keys.iter().any(|k| k == key))
        };

        // ONE sync body over whichever item source the focus names.
        // Reactive property this buys: signal reads are dynamic per run,
        // so in Agent focus this effect tracks ONLY fold+focus(+details/
        // theme/images) — background convo/poller updates never wake it;
        // in Entity focus any convo write re-runs it and the fingerprint
        // fast path no-ops (trivial at chat scale).
        let sync_items = |items: &[Item]| {
            let mut st = state.borrow_mut();
            let slots = cycle_slots(items, &open_cycles, details);
            let mut rebuild = theme_id != st.theme
                || details != st.details
                || focus != st.focus
                || items.len() < st.seen.len();
            if !rebuild {
                // A mid-list visibility flip cannot be expressed with
                // keyed appends (order is push order) — rebuild instead.
                for (i, item) in items.iter().enumerate().take(st.seen.len()) {
                    let visible = is_visible(item, details, &slots[i]);
                    let body = visible && has_body(item, &slots[i]);
                    if st.seen[i].1 != visible || st.bodies.get(i).copied().unwrap_or(false) != body
                    {
                        rebuild = true;
                        break;
                    }
                }
            }
            if rebuild {
                st.theme = theme_id;
                st.details = details;
                st.focus = focus.clone();
                st.seen.clear();
                st.bodies.clear();
                feed.clear();
                for (i, item) in items.iter().enumerate() {
                    let ex = is_expanded(item);
                    let fp = fingerprint(item, &store, ex, &slots[i]);
                    match render_item(&t, item, store, details, link_root.as_ref(), ex, &slots[i]) {
                        Some(fi) => {
                            feed.push(format!("i{i}"), fi);
                            st.seen.push((fp, true));
                            let body = render_body(&t, item, &slots[i]);
                            st.bodies.push(body.is_some());
                            if let Some(b) = body {
                                feed.push(format!("i{i}~body"), b);
                            }
                        }
                        None => {
                            st.seen.push((fp, false));
                            st.bodies.push(false);
                        }
                    }
                }
                return;
            }
            for (i, item) in items.iter().enumerate() {
                let known = i < st.seen.len();
                let ex = is_expanded(item);
                let fp = fingerprint(item, &store, ex, &slots[i]);
                if known && st.seen[i] == (fp, is_visible(item, details, &slots[i])) {
                    continue; // unchanged
                }
                match render_item(&t, item, store, details, link_root.as_ref(), ex, &slots[i]) {
                    Some(fi) => {
                        // Existing key -> in-place replace; new key ->
                        // append (source items only ever append, so a new
                        // visible key always lands at the tail). The body
                        // entry follows its item the same way (a body
                        // appearing or vanishing on a KNOWN item rebuilt
                        // above).
                        feed.push(format!("i{i}"), fi);
                        let body = render_body(&t, item, &slots[i]);
                        let has = body.is_some();
                        if let Some(b) = body {
                            feed.push(format!("i{i}~body"), b);
                        }
                        if known {
                            st.seen[i] = (fp, true);
                            st.bodies[i] = has;
                        } else {
                            st.seen.push((fp, true));
                            st.bodies.push(has);
                        }
                    }
                    None => {
                        if known {
                            st.seen[i] = (fp, false);
                            st.bodies[i] = false;
                        } else {
                            st.seen.push((fp, false));
                            st.bodies.push(false);
                        }
                    }
                }
            }
        };

        match &focus {
            Focus::Agent => {
                // Image loads re-render image items (fingerprints read
                // entries; only the agent fold renders images) — reading
                // it in this arm only keeps entity focus untracked.
                let _ = store.images.with(|v| v.len());
                store.fold.with(|f| sync_items(&f.items));
            }
            Focus::Entity(name) => {
                store.convos.with(|cs| {
                    match cs.iter().find(|c| c.name == *name) {
                        Some(c) => sync_items(&c.items),
                        // Focus names a conversation that does not exist
                        // (should not happen; render honestly as empty).
                        None => sync_items(&[]),
                    }
                });
            }
        }
    });
}

// ---------------------------------------------------------------------------
// A read-only transcript (an automation's runs as a chat)
// ---------------------------------------------------------------------------

/// One entry of a read-only transcript: a full-width separator (its feed
/// key, its label) or a transcript item.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatEntry {
    Separator { key: String, label: String },
    Item(Box<Item>),
}

/// The conversation's transcript widget over fixed entries, read-only:
/// the same `render_item` cards (full detail: nothing capped), a
/// separator per entry group (`rule_block`), wrapped to the width,
/// scrolled by the engine `Scroll` (↑↓, PgUp/PgDn, Home/End, the wheel;
/// it takes the focus). `focus_key` = the separator to show first (else
/// the end, the newest).
pub fn read_only_transcript(
    cx: Scope,
    store: Store,
    entries: &[ChatEntry],
    focus_key: Option<String>,
) -> View {
    let t = abstracttui::app::current_theme().tokens;
    let feed = FeedState::new(cx);
    for (i, e) in entries.iter().enumerate() {
        match e {
            ChatEntry::Separator { key, label } => feed.push(
                key.clone(),
                FeedItem::new().block(rule_block('━', label.clone(), t.accent, t.text_faint)),
            ),
            ChatEntry::Item(item) => {
                // No cycle panels here: every item stands on its own.
                if let Some(fi) = render_item(&t, item, store, true, None, false, &Slot::Plain) {
                    feed.push(format!("c{i}"), fi);
                    // The answer's markdown is its own entry, after its rule
                    // (the live transcript's `i{i}~body`).
                    if let Some(b) = render_body(&t, item, &Slot::Plain) {
                        feed.push(format!("c{i}~body"), b);
                    }
                }
            }
        }
    }
    let offset = cx.signal(0i32);
    let follow = cx.signal(focus_key.is_none());
    if let Some(key) = focus_key {
        // Once laid out, open on the chosen run (then the reader scrolls).
        let feed2 = feed.clone();
        let done = Rc::new(std::cell::Cell::new(false));
        cx.effect(move || {
            let _ = feed2.total_rows().get();
            if done.get() {
                return;
            }
            if let Some(row) = feed2.row_of(&key) {
                done.set(true);
                offset.set(row);
            }
        });
    }
    abstracttui::widgets::Scroll::new(Feed::new(&feed).gap(0).view(cx))
        .offset_y(offset)
        .follow_tail(follow)
        .layout(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)))
        .element(cx, &t)
        .focusable()
        .autofocus()
        .build()
}

// ---------------------------------------------------------------------------
// Live replies (contract S): a SECOND feed under the transcript
// ---------------------------------------------------------------------------

/// Header item for one live bubble: a blank row, the caption, the state,
/// then (when present) the collapsed reasoning line and the truncation
/// note. Rebuilt per header change only — never per content delta.
pub fn live_header_item(t: &TokenSet, e: &crate::live::LiveReply) -> FeedItem {
    let caption = if e.child {
        format!("sub-agent · {}", e.node_id)
    } else if e.node_id.is_empty() {
        "reply".to_string()
    } else {
        format!("reply · {}", e.node_id)
    };
    let state = if e.finished {
        "complete — the recorded reply replaces this"
    } else {
        "streaming…"
    };
    let mut lines = vec![
        RichLine::new(),
        header_line("✎", t.accent, &caption, t.text_muted, state, t.text_faint),
    ];
    if !e.reasoning.is_empty() {
        lines.push(RichLine::from_spans(vec![Span::new(
            live_reasoning_line(&e.reasoning),
            Style::new().fg(t.text_faint),
        )]));
    }
    if e.truncated {
        lines.push(RichLine::from_spans(vec![Span::new(
            "[#TRUNCATION] the gateway dropped the start of this live text (its per-call buffer is full); the full reply appears when the call completes",
            Style::new().fg(t.warn),
        )]));
    }
    FeedItem::new().rich_block(RichText::from_lines(lines))
}

/// The collapsed reasoning line: its NEWEST line, cut to 100 chars with
/// the cut said, plus the total size. The full reasoning lands in the
/// cycle card when the call's record arrives; it is never mixed into the
/// reply text.
pub fn live_reasoning_line(reasoning: &str) -> String {
    const SHOWN: usize = 100;
    let total = reasoning.chars().count();
    let last = reasoning
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let n = last.chars().count();
    let tail: String = if n > SHOWN {
        //[WARNING:TRUNCATION] live reasoning gist; the full text lands in the cycle card
        let t: String = last.chars().skip(n - SHOWN).collect();
        format!("…{t}")
    } else {
        last.to_string()
    };
    format!("  ∴ thinking: {tail}  ({total} chars of reasoning so far)")
}

/// Keep `live_feed` matched to `store.live`. Per content delta the ONLY
/// work is one `stream_append` of the new text (the engine re-typesets
/// just the open tail region); the main transcript feed is never touched.
/// Structural changes — a bubble added or retired, a snapshot replacing a
/// call's text, a theme switch, focus leaving the agent lane — clear and
/// rebuild this small feed (it holds only the calls in flight).
///
/// Visible only for the agent lane's CURRENT, unfinished turn: a bubble
/// never outlives the turn's final answer or crosses into another run
/// (the fold's root/finished state gates it, so a reset or a session
/// switch hides stale bubbles even before the live state is rebound).
pub fn wire_live_feed(cx: Scope, store: Store, live_feed: &FeedState) {
    #[derive(Default)]
    struct LiveSync {
        /// (call_id, content_gen) per rendered bubble, in feed order.
        keys: Vec<(String, u64)>,
        /// Content bytes already appended, per bubble.
        lens: Vec<usize>,
        /// (header fingerprint, finished) per bubble.
        heads: Vec<(u64, bool)>,
        theme: &'static str,
    }
    let state = Rc::new(RefCell::new(LiveSync::default()));
    let feed = live_feed.clone();
    cx.effect(move || {
        let theme = abstracttui::app::use_theme(cx).get();
        let t = theme.tokens;
        let agent = matches!(store.focus.get(), Focus::Agent);
        let (root, finished) = store
            .fold
            .with(|f| (f.root_run_id().to_string(), f.finished));
        let entries: Vec<crate::live::LiveReply> = store.live.with(|l| {
            if agent && !finished && l.root() == root {
                l.entries().to_vec()
            } else {
                Vec::new()
            }
        });
        let head_fp = |e: &crate::live::LiveReply| -> (u64, bool) {
            let mut h = Fnv::new();
            h.str(&e.node_id);
            h.byte(u8::from(e.child));
            h.byte(u8::from(e.truncated));
            h.byte(u8::from(e.finished));
            h.body(&live_reasoning_line(&e.reasoning));
            (h.finish(), e.finished)
        };
        let mut st = state.borrow_mut();
        let keys: Vec<(String, u64)> = entries
            .iter()
            .map(|e| (e.call_id.clone(), e.content_gen))
            .collect();
        if keys != st.keys || theme.id != st.theme {
            feed.clear();
            st.theme = theme.id;
            st.keys = keys;
            st.lens.clear();
            st.heads.clear();
            for e in &entries {
                feed.push(format!("h:{}", e.call_id), live_header_item(&t, e));
                let sk = format!("s:{}", e.call_id);
                feed.push_stream(sk.clone());
                if !e.content.is_empty() {
                    feed.stream_append(&sk, &e.content);
                }
                if e.finished {
                    feed.stream_finish(&sk);
                }
                st.lens.push(e.content.len());
                st.heads.push(head_fp(e));
            }
            return;
        }
        for (i, e) in entries.iter().enumerate() {
            let sk = format!("s:{}", e.call_id);
            if e.content.len() > st.lens[i] {
                // Same generation = append-only text: the new bytes are
                // exactly the suffix past what the feed already holds.
                feed.stream_append(&sk, &e.content[st.lens[i]..]);
                st.lens[i] = e.content.len();
            }
            let fp = head_fp(e);
            if fp != st.heads[i] {
                feed.update(&format!("h:{}", e.call_id), live_header_item(&t, e));
                if fp.1 && !st.heads[i].1 {
                    feed.stream_finish(&sk);
                }
                st.heads[i] = fp;
            }
        }
    });
}

/// Mirror of `render_item`'s hide rules (cheap, no rendering). The sync
/// effect's ORDER correctness depends on this mirror staying exact —
/// `render_item` returning `Some` for an item this predicate calls
/// hidden would append a mid-list key at the feed tail (feed order is
/// push order). Pinned by `tests::visibility_mirror_matches_render_item`.
fn is_visible(item: &Item, details: bool, slot: &Slot) -> bool {
    // A call inside a FOLDED cycle panel is summarised on the cycle row.
    if matches!(slot, Slot::Member { open: false }) {
        return false;
    }
    match item {
        // Entity probe bodies stay details-gated; thinking and tools
        // are ALWAYS visible (operator directives 2026-07-26 and
        // 2026-08-19) — /details gates their VERBOSITY in
        // `render_item`, never their existence.
        Item::Probe { .. } => details,
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// The pane
// ---------------------------------------------------------------------------

/// The transcript pane: `Scroll` over the feed, engine follow-tail.
/// Rebuilt only when the empty/connection state flips (memoized in the
/// ROOT — one predicate shared with the splash ticker) or the theme
/// changes — item traffic never remounts the scroll.
///
/// "Empty" = no CONVERSATION yet: boot pushes Info notices (session id,
/// workspace policy) into the fold, and gating on `items.is_empty()`
/// made the guidance unreachable on every normal launch (adversary P2,
/// 2026-07-22). Info-only folds show the guidance WITH the notices
/// below it — never instead of them.
///
/// `splash` is read ONLY inside the empty and restoring branches
/// (conditional signal tracking): once conversation shows, the pane
/// carries no frame dependency and animation ticks can never remount
/// the Scroll.
#[allow(clippy::too_many_arguments)]
pub fn pane(
    cx: Scope,
    t: &TokenSet,
    store: Store,
    ctx: &crate::ui::UiCtx,
    feed: &FeedState,
    live_feed: &FeedState,
    offset: Signal<i32>,
    follow: Signal<bool>,
    empty: abstracttui::reactive::Memo<bool>,
    splash: Signal<u64>,
    anim: crate::ui::animation::FeedHandle,
    anim_frame: Signal<u64>,
    overlay_open: abstracttui::reactive::Memo<bool>,
) -> View {
    let tokens = *t;
    let feed = feed.clone();
    let live_feed = live_feed.clone();
    let gateway_label = ctx.gateway_label.clone();
    let workspace_root = ctx.workspace_root.clone().unwrap_or_default();
    // Right-click context menu inputs (operator ask, 2026-08-28): the
    // overlay store the engine popup opens in, and the linkify root for
    // the menu's "Copy path" row.
    let overlays = ctx.overlays.clone();
    let link_root: Option<Rc<str>> = ctx.workspace_root.as_deref().map(Rc::from);
    // The message play/pause icons read through the voice lane.
    let speak_ctx = ctx.clone();
    // `basis(Cells(0))` beside `grow(1.0)` (2026-08-20): without it this
    // wrapper's basis is AUTO — measured from its content, i.e. the whole
    // transcript — so the chrome column overflowed by hundreds of rows the
    // moment a single item existed, and the solver's shrink pass bought a
    // row back from the composer (4 rows requested, 3 drawn, caret clipped:
    // typing past the visible rows went blind). This is the same class the
    // engine closed for `Scroll` itself, whose default layout IS
    // `grow(1.0).basis(Cells(0))` (abstracttui 0240 follow-up #1) — but the
    // default dies at this wrapper, because an auto-sized ancestor re-derives
    // a content-sized basis from the Scroll inside it. Measured: with the
    // basis, the composer keeps its 4 rows at every viewport height and
    // transcript length; without it, exactly one row is lost from ONE item on.
    dyn_view_scoped(
        LayoutStyle::column()
            .grow(1.0)
            .basis(Dimension::Cells(0))
            .padding(Edges::hv(1, 0)),
        move |scx| {
            // `/animation` outranks both branches: it is an explicit act,
            // and it replaces the pane only — chrome, composer, approvals
            // and notices all stay live behind it.
            if store.animation.get() > 0 {
                return crate::ui::animation::pane(scx, store, anim.clone(), anim_frame);
            }
            // A session restore in flight (boot --resume, a /sessions
            // pick): the pane IS the waiting surface — the animated
            // loading screen (operator ask, 2026-08-28). Ordered ABOVE
            // the empty branch: the probe's fold swap can land a frame
            // before `restoring` clears, and that frame must stay the
            // loading screen, never flash the splash's "describe a
            // task" over a session whose history just arrived.
            //
            // AGENT LANE ONLY (adversarial review 2026-08-28, E): the
            // restore belongs to the agent conversation, and nothing
            // stops the operator from opening an entity lane during it
            // (Alt+E, `/focus`, `@name` — none of them consult
            // `restoring`). Ungated, the pane claimed "restoring
            // session <agent-session>…" over an entity conversation
            // that has nothing to do with the restore, while the
            // composer below it showed that entity's own placeholder.
            // The empty chat (splash, restore screen) is BACKGROUND: while
            // anything is open over it, nothing of it is drawn — no logo,
            // no mark, no guidance around or through the overlay.
            if empty.get() && overlay_open.get() {
                return Element::new()
                    .style(LayoutStyle::default().grow(1.0))
                    .build();
            }
            if store.restoring.get() && matches!(store.focus.get(), Focus::Agent) {
                return crate::ui::loading::view(&tokens, store, splash.get());
            }
            if empty.get() {
                let conn = store.conn.get();
                let frame = splash.get();
                // Read the fold ONLY on this branch: while the guidance
                // shows, new notices re-render it; once conversation
                // starts, the pane carries no fold dependency at all.
                let mut notices: Vec<String> = store.fold.with(|f| {
                    f.items
                        .iter()
                        .filter_map(|i| match i {
                            Item::Info { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect()
                });
                // Bounded: the splash renders notices UNCLAMPED (no
                // Scroll on this branch) — a notice flood overflows the
                // column and flex-crushes the chrome rows below to zero
                // height (found by the visibility wave's own test: the
                // activity strip vanished under 40 notices). Newest
                // survive; the count says what folded.
                if notices.len() > 8 {
                    let hidden = notices.len() - 8;
                    notices = notices.split_off(hidden);
                    notices.insert(0, format!("(+{hidden} earlier notices)"));
                }
                return empty_state(
                    &tokens,
                    store,
                    &conn,
                    &gateway_label,
                    &workspace_root,
                    &notices,
                    frame,
                );
            }
            // Right-click on an item opens its action menu
            // (`ui::item_menu`). A wrapper element rather than a Feed
            // hook: the engine's Feed exposes LEFT press only (its
            // `on_item_press`), so this hears the secondary press on
            // bubble and maps the row back through
            // `FeedState::item_at_row`.
            //
            // The wrapper sits INSIDE the Scroll, wrapping the Feed
            // (adversarial review 2026-08-28, C). Outside it, the row
            // needed a `+ offset` term — and that term is WRONG while a
            // run streams into a scrolled transcript: `Scroll`
            // bottom-anchors its content whenever the tail is pinned and
            // syncs the bound `offset_y` only a turn later ("the pin IS
            // the position", scroll.rs), so the signal lags the drawn
            // position by one append and the menu opened on an EARLIER
            // item — then copied it under a notice that sounded right.
            // Inside, the wrapper's own rect IS the scrolled content
            // rect, so `pos.y - rect.y` is the content row directly:
            // the engine's own formula in `Feed::on_item_press`, immune
            // to the pin lag by construction.
            let ui_feed = feed.clone();
            let speak_ctx = speak_ctx.clone();
            let menu_overlays = overlays.clone();
            let menu_root = link_root.clone();
            let content = Element::new()
                .style(LayoutStyle::column().width(Dimension::Percent(1.0)))
                .on(abstracttui::ui::Phase::Bubble, move |ectx, ev| {
                    let abstracttui::ui::UiEvent::Mouse(m) = ev else {
                        return;
                    };
                    // LEFT press: the disclosure marker, and ONLY its
                    // cells. Screen text-selection is enabled app-wide
                    // (`lib.rs`), and the engine's own
                    // `Feed::on_item_press` stops propagation for any
                    // press that lands on an item — wiring that would
                    // have killed drag-to-select across the whole
                    // transcript. Consuming a ~11-cell right-aligned
                    // target instead leaves every other cell selectable.
                    if let abstracttui::ui::MouseKind::Down(abstracttui::ui::MouseButton::Left) =
                        m.kind
                    {
                        let rect = ectx.current_rect();
                        if let Some(text) = speak_icon_at(store, &ui_feed, rect, m.pos) {
                            crate::ui::voice_view::toggle_message(store, &speak_ctx, &text);
                            ectx.stop_propagation();
                            return;
                        }
                        if toggle_cycle_at(store, &ui_feed, follow, offset, rect, m.pos)
                            || toggle_marker_at(store, &ui_feed, rect, m.pos)
                        {
                            ectx.stop_propagation();
                        }
                        return;
                    }
                    let abstracttui::ui::MouseKind::Down(abstracttui::ui::MouseButton::Right) =
                        m.kind
                    else {
                        return;
                    };
                    let row = m.pos.y - ectx.current_rect().y;
                    let Some((key, _)) = ui_feed.item_at_row(row) else {
                        return; // a gap or past-the-end press affords nothing
                    };
                    let Some(ix) = item_index(&key) else {
                        return;
                    };
                    // Agent lane only: entity conversations use other
                    // item sources under the same keys.
                    if !matches!(store.focus.get_untracked(), Focus::Agent) {
                        return;
                    }
                    let Some(item) = store.fold.with_untracked(|f| f.items.get(ix).cloned()) else {
                        return;
                    };
                    let actions = crate::ui::item_menu::items_for(&item, menu_root.as_deref());
                    // Nothing to act on is SAID, never silent (review I-2):
                    // `ContextMenu::open` also returns None for an
                    // all-disabled menu, and the press is consumed either
                    // way — an unexplained dead right-click reads as a
                    // flaky menu, since the same card affords actions the
                    // moment its result lands.
                    let inert = actions.is_empty() || actions.iter().all(|a| a.disabled);
                    ectx.stop_propagation();
                    if inert {
                        store.notify("nothing to copy from this card yet");
                        return;
                    }
                    let act_root = menu_root.clone();
                    let opened = abstracttui::app::ContextMenu::new(actions)
                        .access_label("transcript item actions")
                        .overlays(&menu_overlays)
                        .on_action(move |k| {
                            crate::ui::item_menu::act(store, &item, k, act_root.as_deref())
                        })
                        .open(cx, m.pos);
                    if opened.is_none() {
                        // No overlay context, or a viewport with no room.
                        store.notify("no room to open the item menu here");
                    }
                })
                .child(Feed::new(&feed).gap(0).view(scx))
                // Live replies (contract S) render BELOW the transcript in
                // their own feed: a token appends there, never re-laying
                // out the transcript above it.
                .child(Feed::new(&live_feed).gap(0).view(scx))
                .build();
            // gap 0: spacing lives INSIDE items (`gap_row`), so tool
            // rows pack under their cycle while sections keep air.
            abstracttui::widgets::Scroll::new(content)
                .offset_y(offset)
                .follow_tail(follow)
                .layout(LayoutStyle::default().grow(1.0))
                .element(scx, &tokens)
                .build()
        },
    )
}

/// A left press on a message's play/pause icon: the message's text, to
/// read / pause / resume (`voice_view::toggle_message`). `None` for every
/// other cell — the icon zone is the ONLY control, so selection keeps
/// working on the rest of the row. Geometry mirrors [`paint_icon`]: the
/// zone's three cells at the right edge, on the message's top row (row 1
/// of both the user card and the answer rule; row 0 is the air above).
fn speak_icon_at(
    store: Store,
    feed: &FeedState,
    rect: abstracttui::base::Rect,
    pos: Point,
) -> Option<String> {
    if rect.w < ICON_ZONE + 8 {
        return None;
    }
    let x0 = rect.right() - ICON_ZONE;
    if pos.x < x0 || pos.x >= x0 + 3 {
        return None;
    }
    let (key, within) = feed.item_at_row(pos.y - rect.y)?;
    // The icon lives on the item's own entry, never on its body entry.
    if within != 1 || key.contains('~') {
        return None;
    }
    let ix = item_index(&key)?;
    let text_at = |items: &[Item]| items.get(ix).and_then(speakable).map(str::to_string);
    match store.focus.get_untracked() {
        Focus::Agent => store.fold.with_untracked(|f| text_at(&f.items)),
        Focus::Entity(name) => store.convos.with_untracked(|cs| {
            cs.iter()
                .find(|c| c.name == name)
                .and_then(|c| text_at(&c.items))
        }),
    }
}

/// A left press on a cycle panel's HEADER ROW opens or folds that cycle
/// (operator ask 2026-10-10). Returns whether the press was CONSUMED —
/// only the header row is a control; every body row stays selectable.
/// Inert under `/details full`, where every cycle is open and the rows
/// are not toggles (the global mode outranks the per-panel one).
///
/// Works in whichever conversation has focus: the feed renders the
/// focused item source, so the hit row is resolved against that same
/// source.
fn toggle_cycle_at(
    store: Store,
    feed: &FeedState,
    follow: Signal<bool>,
    offset: Signal<i32>,
    rect: abstracttui::base::Rect,
    pos: Point,
) -> bool {
    if store.show_details.get_untracked() {
        return false;
    }
    let Some((key, within)) = feed.item_at_row(pos.y - rect.y) else {
        return false;
    };
    // The header is on the item's own entry; its body entry (the panel's
    // words) is text, never a toggle.
    if key.contains('~') {
        return false;
    }
    let Some(ix) = item_index(&key) else {
        return false;
    };
    // The item and its slot from the SAME computation the sync used, so
    // the hit row and the drawn row cannot disagree (a folded row after
    // an open panel carries a lead blank).
    let open_cycles = store.expanded_cycles.get_untracked();
    let at = |items: &[Item]| -> Option<Slot> {
        cycle_slots(items, &open_cycles, false).get(ix).cloned()
    };
    let hit = match store.focus.get_untracked() {
        Focus::Agent => store.fold.with_untracked(|f| at(&f.items)),
        Focus::Entity(name) => store.convos.with_untracked(|cs| {
            cs.iter()
                .find(|c| c.name == name)
                .and_then(|c| at(&c.items))
        }),
    };
    let Some(Slot::Head {
        open,
        lead,
        key: cycle,
        base,
        ..
    }) = hit
    else {
        return false;
    };
    if within != CycleHeader::header_row(open, lead) {
        return false;
    }
    // Folding removes BOTH forms (a cycle opened before its first call
    // landed was stored under its base key); opening stores the full one.
    store.expanded_cycles.update(|e| {
        if open {
            e.retain(|k| *k != cycle && *k != base);
        } else {
            e.push(cycle);
        }
    });
    // Release follow-tail: with the tail pinned, a panel growing in
    // place pushed everything above it UP and the clicked header off the
    // top of the viewport (adversarial review 2026-10-10). Opening also
    // scrolls the header to the TOP of the view (one row of context above
    // it), so the panel reads downward from it instead of unfolding below
    // the visible area when the row sat near the bottom. Folding keeps
    // the view where it is. The press row IS the header's content row:
    // `rect` is the scrolled content rect (see the handler in `pane`).
    follow.set(false);
    if !open {
        offset.set((pos.y - rect.y - 1).max(0));
    }
    true
}

/// A left press on a folded tool card's disclosure marker toggles that
/// card's output. Returns whether the press was CONSUMED — false for
/// every other cell, which is what keeps drag-to-select alive over the
/// rest of the transcript.
///
/// The hit box is derived from [`tool_marker`], the same function the
/// row draws with, so the target can never drift from the glyph. The
/// row's own geometry supplies the rest: the marker is right-aligned at
/// `rect.right() - width`, on the item's FIRST row.
fn toggle_marker_at(
    store: Store,
    feed: &FeedState,
    rect: abstracttui::base::Rect,
    pos: Point,
) -> bool {
    // Agent lane only: entity conversations key other item sources
    // under the same `i{index}` feed keys.
    if !matches!(store.focus.get_untracked(), Focus::Agent) {
        return false;
    }
    // `/details` shows every body already — there is no marker to hit.
    if store.show_details.get_untracked() {
        return false;
    }
    let Some((key, within)) = feed.item_at_row(pos.y - rect.y) else {
        return false;
    };
    // Row 0 of the item is the tool row; the error and body rows below
    // it carry no control.
    if within != 0 {
        return false;
    }
    let Some(ix) = item_index(&key) else {
        return false;
    };
    // Only a PLAIN tool row draws the marker: a call inside a cycle panel
    // is either hidden (folded cycle) or a full card (open cycle), and a
    // full card's row 0 is its blank lead — pressing there must stay a
    // text-selection press.
    let open_cycles = store.expanded_cycles.get_untracked();
    let plain = store.fold.with_untracked(|f| {
        cycle_slots(&f.items, &open_cycles, false)
            .get(ix)
            .is_some_and(|s| *s == Slot::Plain)
    });
    if !plain {
        return false;
    }
    let Some((tool_key, marker, name, status)) =
        store.fold.with_untracked(|f| match f.items.get(ix) {
            Some(Item::Tool {
                key,
                name,
                status,
                result,
                ..
            }) => {
                let expanded = store
                    .expanded_tools
                    .with_untracked(|e| e.iter().any(|k| k == key));
                tool_marker(&shown_result(result), *status, expanded, rect.w)
                    .map(|m| (key.clone(), m, name.clone(), *status))
            }
            _ => None,
        })
    else {
        return false;
    };
    // THE SAME layout the row drew with — including its `marker_fits`
    // term. Without it the hit box existed on rows that drew no marker
    // (measured: at 34 columns a click on the args hint expanded the
    // body and swallowed the text-selection press, with nothing on
    // screen to explain either).
    let word_seg = format!("· {}", tool_status_word(status));
    let lay = tool_row_layout(
        rect.w,
        tool_glyph_str(status),
        &name,
        &word_seg,
        Some(marker.as_str()),
    );
    if !lay.marker_fits {
        return false;
    }
    let mx = rect.x + lay.marker_x;
    if pos.x < mx || pos.x >= rect.right() {
        return false;
    }
    // Toggle by the STABLE tool key, never the positional feed key.
    store
        .expanded_tools
        .update(|e| match e.iter().position(|k| *k == tool_key) {
            Some(i) => {
                e.remove(i);
            }
            None => e.push(tool_key),
        });
    true
}

/// The boot/idle identity card's rows (IDLE-1) — the Python banner's
/// fact set as label/value pairs. Pure over signal reads so the same
/// builder can serve a future `/status` command (W2 R-CMD-5). Reads
/// signals: call inside a reactive scope.
pub fn status_card_rows(
    store: Store,
    gateway_label: &str,
    workspace_root: &str,
) -> Vec<(&'static str, String)> {
    let signed_out = store.signed_out.get();
    let workflow = store.workflow.with(|w| {
        if w.flow_id.is_empty() && signed_out.is_some() {
            "unknown — not signed in to the gateway".to_string()
        } else if w.flow_id.is_empty() {
            "none yet — /workflow picks one".to_string()
        } else {
            w.display_label()
        }
    });
    let route = crate::ui::chrome::route_label(store);
    let mode = {
        let m = store.workspace_mode.get();
        if m.trim().is_empty() {
            "server-managed (gateway policy) — /workspace".to_string()
        } else {
            format!("{} — /workspace", m.trim())
        }
    };
    let session = store.session_id.get();
    let conn_word = match store.conn.get() {
        crate::store::Conn::Ok if signed_out.is_some() => "reachable · not signed in",
        crate::store::Conn::Ok => "connected",
        crate::store::Conn::Unknown => "probing…",
        // Evidence-based words (HOLE A): "unreachable" only on connect-level
        // proof; a threshold of timeouts against a live-but-busy gateway says
        // "not responding" — the operator's gateway WAS reachable when the
        // old wording claimed otherwise.
        crate::store::Conn::Down(_, true) => "unreachable",
        crate::store::Conn::Down(_, false) => "not responding",
    };
    let skills = store.selected_skills.with(|s| {
        if s.is_empty() {
            "none attached — /skills".to_string()
        } else {
            format!("{} ({} attached)", s.join(", "), s.len())
        }
    });
    let mcp = store.mcp_servers.with(|m| {
        if m.is_empty() {
            "none registered — /mcp".to_string()
        } else {
            let names: Vec<&str> = m.iter().map(|s| s.name.as_str()).collect();
            format!("{} ({})", names.join(", "), names.len())
        }
    });
    let context = {
        let w = store.context_window.get();
        if w == 0 {
            "window not declared — /context <tokens> enables the % meter".to_string()
        } else {
            format!(
                "{} tk window (declared — /context)",
                crate::ui::chrome::fmt_tokens(w)
            )
        }
    };
    let mut rows: Vec<(&'static str, String)> = vec![
        (
            "version",
            format!("{} · rendered by AbstractTUI", crate::cli::VERSION),
        ),
        ("workflow", workflow),
        ("route", route),
    ];
    // Gating row appears only when the operator has chosen unattended —
    // gated is the default and needs no line; ungated must NEVER be a
    // silent surprise (the whole point of showing it).
    if store.gating_mode.get() == "auto" {
        rows.push((
            "gating",
            "auto — UNATTENDED (approval pauses skipped; /gating wait re-gates)".to_string(),
        ));
    }
    if !workspace_root.is_empty() {
        rows.push(("cwd", workspace_root.to_string()));
    }
    rows.push(("workspace", mode));
    rows.push(("session", session));
    rows.push(("gateway", format!("{gateway_label} · {conn_word}")));
    rows.push(("skills", skills));
    rows.push(("mcp", mcp));
    rows.push(("context", context));
    rows
}

/// Boot/idle identity card (IDLE-1) under the splash logo (IDLE-2): the
/// first frame reads as a cockpit, not an empty prompt — 27 of 36 rows
/// were blank at boot and the most valuable facts (model, workspace,
/// capabilities) were absent or 10s late (review-current-state
/// §4.1/§5).
///
/// CENTERED as one block (operator ask, 2026-07-23 — this REVERSES the
/// earlier top-anchor decision, honestly re-argued): the top anchor
/// existed against the §4.5 ghost class — center-justified content
/// re-seats every row when total height changes (a late boot notice),
/// and the pre-0.2.6 engine's damage gaps could leave stale pixels
/// behind on real terminals. That defense is retired on evidence, not
/// forgotten: the engine's damage contract hardened across 0.2.x, an
/// externally damaged screen now self-heals (focus-gained full redraw +
/// Ctrl+L, both engine-owned since 0.2.6), re-seats only happen when a
/// NOTICE arrives (rare, one-shot, model-damaged so the diff re-emits
/// correctly), and the splash animation re-paints the block every
/// ~150ms anyway while it is visible. The opaque ground fill stays as
/// defense-in-depth for within-damage repaints.
///
/// HEIGHT degradation is honest by construction: every content row is
/// `shrink(0.0)` and the outer column clips, so a pane shorter than
/// the block top-aligns and drops BOTTOM rows whole (notices first,
/// hints next) — never the flex-shrink overprint the refinement pass
/// caught at 72×20 ("sessionce…" interleavings), and never the logo.
#[allow(clippy::too_many_arguments)]
fn empty_state(
    t: &TokenSet,
    store: Store,
    conn: &crate::store::Conn,
    gateway_label: &str,
    workspace_root: &str,
    notices: &[String],
    frame: u64,
) -> View {
    // The ENTRANCE (2026-08-21): the boot animation fades its
    // composition out to this exact ground, and the first app screen
    // fades in off it over the same half second — the cut between the
    // two used to be one hard frame, which read as a glitch rather than
    // an arrival. `frame` resets to 0 on every splash entrance, so a
    // return to the idle screen mid-session replays the same soft
    // arrival. One curve, in `logo::boot_fade`.
    let fade = crate::ui::logo::boot_fade(frame);
    let dim = |c: Rgba| crate::ui::logo::lerp_ink(t.bg, c, fade);
    let muted = dim(t.text_muted);
    let faint = dim(t.text_faint);
    let text_ink = dim(t.text);
    let error = dim(t.error);
    // Every content row is shrink(0.0) and the outer column CLIPS
    // (refinement-pass P1, the engine's 0240 class relearned: default-
    // shrink fixed rows on a too-short pane crush to zero height but
    // still paint at the surviving rows' y — at 72×20 the card rendered
    // "sessionce…" interleavings. With shrink pinned + clip, the
    // spacers collapse first, the block top-aligns, and the BOTTOM rows
    // clip away whole — the logo is the last casualty, never the first).
    let line = |s: String, ink: Rgba| {
        Element::new()
            .style(LayoutStyle::line(1).shrink(0.0))
            .draw(move |canvas, rect| {
                // Ellipsize to the pane: a long boot notice used to run
                // off the 80-col edge mid-word (live review, 2026-07-22).
                let fitted = text::truncate_ellipsis(&s, (rect.w - 2).max(4));
                let w = text::width(&fitted);
                let x = rect.x + ((rect.w - w) / 2).max(0);
                canvas.print(Point::new(x, rect.y), &fitted, ink, Rgba::TRANSPARENT);
            })
            .build()
    };
    let spacer = || {
        Element::new()
            .style(LayoutStyle::default().grow(1.0))
            .build()
    };
    let ground = t.bg;
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
        // Top spacer: with its twin below, the content block floats
        // centered; when the pane is shorter than the content both
        // collapse to zero and the block top-aligns.
        .child(spacer());
    // The animated logo lockup leads both branches (the splash IS the
    // brand moment): mark + wordmark + the version as a faint tagline
    // INSIDE the lockup — brand metadata completes it instead of
    // competing with operational facts inside the card (refinement
    // pass; `status_card_rows` keeps the version row for /status).
    let tagline = format!("{} · rendered by AbstractTUI", crate::cli::VERSION);
    // CONTINUITY (2026-08-21): where the pane can afford the rows, the
    // lockup carries the boot animation's OWN mark at rest — the same
    // letterform the animation just assembled, not a second smaller mark
    // that merely rhymes with it. `hero_rows` returns None on panes
    // where those rows would come out of the fact card, and the compact
    // `▲` lockup stands in unchanged.
    col = col.child(
        match crate::ui::logo::hero_rows(abstracttui::app::current_viewport().h) {
            Some(mark_rows) => crate::ui::logo::hero(t, frame, &tagline, mark_rows, fade),
            None => crate::ui::logo::logo(t, frame, &tagline, fade),
        },
    );
    if let crate::store::Conn::Down(msg, gone) = conn {
        // A dead connection must teach RECOVERY, not "describe a task".
        // The message is already evidence-worded by `GwError`'s Display
        // ("gateway unreachable: …" / "gateway timed out: …") — render it
        // verbatim instead of stamping "unreachable" over a timeout (the
        // old prefix branded a busy-but-alive gateway unreachable). The
        // advice follows the evidence: only a GONE gateway needs starting;
        // a not-responding one is running and likely busy.
        col = col.child(line(abstracttui::text::truncate_ellipsis(msg, 78), error));
        if *gone {
            col = col.child(line("start one:  abstractgateway serve".into(), muted));
        } else {
            col = col.child(line(
                "the gateway is running but slow to answer — it may be busy".into(),
                muted,
            ));
        }
        col = col
            .child(line(
                "diagnose:   abstractcode doctor    connect: abstractcode login".into(),
                muted,
            ))
            .child(line(
                "the app reconnects automatically once the gateway answers".into(),
                faint,
            ));
        return col.child(spacer()).build();
    }
    col = col.child(line(
        "describe a task below — the agent runs durably on the gateway".into(),
        muted,
    ));
    // The fact card: label-aligned rows, centered as a block, packed
    // (no inter-row gap — the card reads as ONE unit; the outer column's
    // gap separates it from the guidance lines). Per-row draw with a
    // fixed label column so values align; the shared `line` centering
    // would jitter rows of different lengths. The version row is
    // FILTERED here (it renders as the logo tagline above); it stays in
    // `status_card_rows` for the future /status surface.
    let rows = status_card_rows(store, gateway_label, workspace_root);
    let mut card = Element::new().style(LayoutStyle::column().shrink(0.0));
    for (label, value) in rows {
        if label == "version" {
            continue;
        }
        let label: String = label.into();
        card = card.child(
            Element::new()
                .style(LayoutStyle::line(1).shrink(0.0))
                .draw(move |canvas, rect| {
                    let w = (rect.w - 2).clamp(20, 76);
                    let x0 = rect.x + ((rect.w - w) / 2).max(0);
                    let fitted_label = text::truncate_ellipsis(&label, 10);
                    canvas.print(
                        Point::new(x0, rect.y),
                        &fitted_label,
                        faint,
                        Rgba::TRANSPARENT,
                    );
                    let vx = x0 + 12;
                    let avail = (x0 + w - vx).max(4);
                    let fitted = text::truncate_ellipsis(&value, avail);
                    canvas.print(Point::new(vx, rect.y), &fitted, text_ink, Rgba::TRANSPARENT);
                })
                .build(),
        );
    }
    col = col.child(card.build());
    // Boot notices (workspace policy, session echoes) render ABOVE the
    // static hints row: on short panes rows clip bottom-up, and a
    // session echo ("buffered guidance dropped") must outlive general
    // teaching — never-a-silent-drop beats rediscoverable help text.
    // PACKED into one sub-column (each notice as a direct child of the
    // gap-1 column read double-spaced; refinement pass).
    if !notices.is_empty() {
        let mut pack = Element::new().style(LayoutStyle::column().shrink(0.0));
        for n in notices {
            pack = pack.child(line(format!("· {n}"), faint));
        }
        col = col.child(pack.build());
    }
    col = col.child(line(
        "/help commands · /workflow agents · /model providers · /theme looks · ? keys".into(),
        faint,
    ));
    col.child(spacer()).build()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The disclosure marker is a MEASUREMENT, not decoration: it
    /// appears only where output actually exists, its number is the
    /// result's own line count, and a running call — which has produced
    /// nothing yet — never claims hidden output.
    #[test]
    fn the_marker_counts_only_output_that_exists() {
        // The gap this closes: a finished call with output.
        let m = tool_marker("a\nb\nc", ToolStatus::Ok, false, 100).expect("marker");
        assert_eq!(m, "▸ 3 lines");
        // Expanded flips the arrow and keeps the count — the row still
        // says how much is on screen.
        assert_eq!(
            tool_marker("a\nb\nc", ToolStatus::Ok, true, 100).as_deref(),
            Some("▾ 3 lines")
        );
        // A call that printed nothing (write_file, mkdir) gets NO
        // marker: a "+0 lines" on a wall of successes is noise.
        assert_eq!(tool_marker("", ToolStatus::Ok, false, 100), None);
        // Still running: output does not exist YET, so claiming it
        // would be the fabrication the marker exists to prevent.
        assert_eq!(tool_marker("a\nb", ToolStatus::Running, false, 100), None);
        // A failed call's output is still output.
        assert!(tool_marker("boom", ToolStatus::Failed, false, 100).is_some());
    }

    /// The number the marker PROMISES is the number expanding
    /// DELIVERS. If these two ever disagree the marker is a lie, so
    /// they are pinned against each other rather than separately.
    #[test]
    fn the_marker_promise_matches_the_expansion() {
        let result = (1..=7)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let row = |expanded: bool| ToolRow {
            rows: RefCell::new(None),
            glyph: "✓",
            glyph_ink: Rgba::TRANSPARENT,
            name: "execute_command".into(),
            name_ink: Rgba::TRANSPARENT,
            hint: "cargo build".into(),
            hint_ink: Rgba::TRANSPARENT,
            word: "ok",
            word_ink: Rgba::TRANSPARENT,
            error: String::new(),
            error_ink: Rgba::TRANSPARENT,
            link_root: None,
            result: result.clone(),
            result_ink: Rgba::TRANSPARENT,
            marker_ink: Rgba::TRANSPARENT,
            status: ToolStatus::Ok,
            expanded,
        };
        assert_eq!(row(false).marker(100).as_deref(), Some("▸ 7 lines"));
        // Collapsed: the body is not drawn.
        assert_eq!(row(false).body_lines(100).len(), 0);
        // Every delivered row wears the tool-OUTPUT gutter.
        assert!(row(true)
            .body_lines(100)
            .iter()
            .all(|l| l.starts_with("│ ")));
    }

    /// The promise holds AT EVERY WIDTH and for hostile bodies.
    ///
    /// The first cut compared them once, at width 100, with 7 short
    /// lines — so it could not see that the marker counted SOURCE lines
    /// while the body counted WRAPPED rows (adversarial review
    /// 2026-08-29, D2): `"\n\n\n"` promised 3 and delivered 0 (wrapping
    /// pops trailing blanks), and one 300-column line promised 1 and
    /// delivered 5. Both sides now read `tool_body_rows`, so parity is
    /// structural — this sweeps widths and shapes to keep it that way.
    #[test]
    fn the_marker_and_the_body_agree_at_every_width() {
        let bodies = [
            "one line",
            "a\nb\nc",
            "\n\n\n",
            "   \n  \n",
            "hello\n\n",
            &"x".repeat(300),
            &format!("{}\nshort\n", "y".repeat(300)),
        ];
        for body in bodies {
            for w in [20, 34, 40, 72, 100, 200] {
                let row = ToolRow {
                    rows: RefCell::new(None),
                    glyph: "✓",
                    glyph_ink: Rgba::TRANSPARENT,
                    name: "execute_command".into(),
                    name_ink: Rgba::TRANSPARENT,
                    hint: String::new(),
                    hint_ink: Rgba::TRANSPARENT,
                    word: "ok",
                    word_ink: Rgba::TRANSPARENT,
                    error: String::new(),
                    error_ink: Rgba::TRANSPARENT,
                    link_root: None,
                    result: body.to_string(),
                    result_ink: Rgba::TRANSPARENT,
                    marker_ink: Rgba::TRANSPARENT,
                    status: ToolStatus::Ok,
                    expanded: true,
                };
                let delivered = row.body_lines(w).len();
                // TWO paths compute this marker: the row's memoised
                // method (what the draw shows) and the free
                // `tool_marker` (what the CLICK hit-test measures its
                // target from). They must agree at every width, or the
                // control sits somewhere other than the glyph — pinned
                // here because sabotaging only the free function left
                // the sweep green.
                assert_eq!(
                    row.marker(w),
                    tool_marker(body, ToolStatus::Ok, true, w),
                    "w={w} body={body:?}: draw and hit-test disagree about the marker"
                );
                match row.marker(w) {
                    Some(m) => {
                        let n: usize = m
                            .split_whitespace()
                            .nth(1)
                            .and_then(|s| s.parse().ok())
                            .expect("marker carries a count");
                        assert_eq!(
                            n, delivered,
                            "w={w} body={body:?}: promised {n}, delivered {delivered}"
                        );
                        // Singular reads as singular.
                        assert!(
                            m.ends_with(if n == 1 { " line" } else { " lines" }),
                            "w={w}: {m}"
                        );
                    }
                    None => assert_eq!(
                        delivered, 0,
                        "w={w} body={body:?}: no marker, so nothing may be delivered"
                    ),
                }
            }
        }
    }

    /// The block's declared HEIGHT covers everything it draws.
    ///
    /// A custom block that under-reports paints over the item below it.
    /// The suite could not see this: deleting `body_lines` from the
    /// height callback left 656 tests green (adversarial review
    /// 2026-08-29, test-quality matrix), because nothing compared the
    /// two. This does.
    #[test]
    fn the_declared_height_covers_every_row_drawn() {
        for (result, error, expanded) in [
            ("", "", false),
            ("a\nb\nc", "", false),
            ("a\nb\nc", "", true),
            ("a\nb\nc", "boom\nsecond line", true),
            (&"z".repeat(400), "e", true),
        ] {
            for w in [30, 72, 120] {
                let row = ToolRow {
                    rows: RefCell::new(None),
                    glyph: "✓",
                    glyph_ink: Rgba::TRANSPARENT,
                    name: "execute_command".into(),
                    name_ink: Rgba::TRANSPARENT,
                    hint: String::new(),
                    hint_ink: Rgba::TRANSPARENT,
                    word: "ok",
                    word_ink: Rgba::TRANSPARENT,
                    error: error.into(),
                    error_ink: Rgba::TRANSPARENT,
                    link_root: None,
                    result: result.to_string(),
                    result_ink: Rgba::TRANSPARENT,
                    marker_ink: Rgba::TRANSPARENT,
                    status: ToolStatus::Ok,
                    expanded,
                };
                // What the draw paints: the row, the errors, the body.
                let drawn = 1 + row.error_lines(w).len() + row.body_lines(w).len();
                // What the block DECLARES — the real arithmetic the
                // `CustomBlock` closure delegates to, not a restatement
                // of it (the first cut re-added the terms itself, which
                // is a tautology: deleting the body term from the
                // closure left the suite green).
                let declared = row.height_at(w);
                assert_eq!(
                    declared as usize, drawn,
                    "w={w} expanded={expanded}: declared {declared}, draws {drawn}"
                );
                // And the body only exists when expanded.
                if !expanded {
                    assert_eq!(row.body_lines(w).len(), 0);
                }
            }
        }
    }

    /// The hit box exists exactly where the marker is DRAWN.
    ///
    /// The draw drops a marker it cannot fit; the hit-test had no such
    /// term, so on a narrow pane a click on ordinary row text expanded
    /// the body and swallowed the press that would have started a text
    /// selection — a control with no glyph (adversarial review
    /// 2026-08-29, D1). Both sides read this one function now.
    #[test]
    fn the_hit_box_exists_only_where_the_marker_is_drawn() {
        let marker = "▸ 3 lines";
        let mut narrow_dropped = false;
        for w in 10..120 {
            let lay = tool_row_layout(w, "✓", "execute_command", "· ok", Some(marker));
            if !lay.marker_fits {
                narrow_dropped = true;
                continue;
            }
            // Drawn: the marker sits flush right and never overlaps the
            // hint the row also printed.
            assert_eq!(lay.marker_x, w - text::width(marker), "w={w}");
            assert!(
                lay.hint_x + lay.hint_avail <= lay.marker_x,
                "w={w}: hint runs into the marker"
            );
        }
        assert!(
            narrow_dropped,
            "some width must be too narrow — otherwise this proves nothing"
        );
        // The specific width the review measured: no marker drawn, so
        // no cell on that row may be a control.
        let lay = tool_row_layout(34, "✓", "execute_command", "· ok", Some(marker));
        assert!(!lay.marker_fits, "34 cols cannot fit name + tag + marker");
    }

    /// Every renderable item shape × details mode: `is_visible` must
    /// answer exactly `render_item(..).is_some()` — a mismatch lets the
    /// sync effect append a mid-list key at the feed tail (feed order is
    /// push order) or strand a visible item as hidden.
    #[test]
    fn visibility_mirror_matches_render_item() {
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = crate::store::Store::create(cx);
            let t = abstracttui::theme::themes()[0].tokens;
            let tool = |status: ToolStatus, error: &str| Item::Tool {
                key: "k".into(),
                name: "execute_command".into(),
                args_preview: "cargo test".into(),
                args_full: String::new(),
                status,
                result: "out".into(),
                error: error.into(),
                sandbox: None,
            };
            let items = vec![
                Item::User { text: "q".into() },
                Item::Steer { text: "s".into() },
                Item::Thinking {
                    iteration: 1,
                    content: "c".into(),
                    reasoning: "r".into(),
                    call: crate::transcript::CallCost::default(),
                },
                tool(ToolStatus::AwaitingApproval, ""),
                tool(ToolStatus::Running, ""),
                tool(ToolStatus::Ok, ""),
                tool(ToolStatus::Ok, "boom"),
                tool(ToolStatus::Failed, "bad"),
                tool(ToolStatus::Denied, ""),
                Item::Assistant {
                    text: "a".into(),
                    final_answer: true,
                },
                Item::Assistant {
                    text: "u".into(),
                    final_answer: false,
                },
                Item::Image {
                    run_id: "r".into(),
                    artifact_id: "art1".into(),
                    label: "img".into(),
                },
                Item::Info { text: "i".into() },
                Item::Error { text: "e".into() },
                Item::Probe {
                    title: "memories in context (2)".into(),
                    body: "[episode] a prior check\n  digest".into(),
                },
            ];
            // Every slot an item can hold: the cycle folded, the cycle
            // opened by key, and the item rendered as if it stood alone.
            let open_key = vec![cycle_keys(&items)[2].clone().expect("a cycle")];
            for details in [true, false] {
                for open in [Vec::new(), open_key.clone()] {
                    let slots = cycle_slots(&items, &open, details);
                    for (item, slot) in items.iter().zip(&slots) {
                        for slot in [slot.clone(), Slot::Plain] {
                            assert_eq!(
                                is_visible(item, details, &slot),
                                render_item(&t, item, store, details, None, false, &slot)
                                    .is_some(),
                                "visibility mirror diverged for {item:?} details={details} slot={slot:?}"
                            );
                        }
                    }
                }
            }
        });
        root.dispose();
    }

    /// Card geometry honesty: the height callback and the draw share
    /// `lines_at`, so the row count must be stable across widths,
    /// including degenerate and unicode inputs (the engine's windowing
    /// trusts the height answer).
    #[test]
    fn wrap_capped_holds_at_degenerate_widths_and_unicode() {
        // Zero/negative width never panics and never returns an
        // unbounded count: either everything fits inside the slack
        // (cap + CAP_SLACK rows) or the cap bites (cap + 1 marker).
        for width in [-5, 0, 1, 2, 4, 10, 80] {
            let (lines, n) = wrap_capped("héllo wörld — ééé 漢字テスト line\nsecond", width, 3);
            assert_eq!(lines.len(), n);
            assert!(
                n <= 3 + CAP_SLACK,
                "cap 3 within slack, got {n} at width {width}"
            );
        }
        // Cap overflow (past the slack) appends exactly one marker line.
        let long = (0..40)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (lines, n) = wrap_capped(&long, 20, 6);
        assert_eq!(n, 7);
        assert!(lines.last().unwrap().contains("more lines"));
        // Slack: a marker never hides fewer rows than it costs — an
        // overflow of ≤ CAP_SLACK renders whole instead (adversarial
        // review round 2: "… (+2 more lines)" spent a row to save two).
        let eight = (0..8)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (lines, n) = wrap_capped(&eight, 20, 6);
        assert_eq!(n, 8, "hidden ≤ slack renders whole: {lines:?}");
        // Tail preservation for result bodies: head, marker, then the
        // LAST rows — the punchline of command output lives at the end.
        let tailed = wrap_capped_tail(&long, 20, 6);
        assert_eq!(tailed.len(), 6, "head 3 + marker + tail 2: {tailed:?}");
        assert!(tailed[3].contains("+35 more lines"), "{tailed:?}");
        assert_eq!(tailed[5], "line 39", "the final line survives: {tailed:?}");
        // Trailing blank lines trim; interior blanks survive.
        let (lines, _) = wrap_capped("a\n\nb\n\n\n", 20, 10);
        assert_eq!(lines, vec!["a".to_string(), String::new(), "b".to_string()]);
    }

    /// The fingerprint must cover every field `render_item` reads for a
    /// tool card — a missed field means a stale card that never repaints.
    #[test]
    fn tool_fingerprint_tracks_every_rendered_field() {
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = crate::store::Store::create(cx);
            let base = Item::Tool {
                key: "k".into(),
                name: "n".into(),
                args_preview: "a".into(),
                args_full: String::new(),
                status: ToolStatus::Running,
                result: "r".into(),
                error: String::new(),
                sandbox: None,
            };
            let fp = |i: &Item| fingerprint(i, &store, false, &Slot::Plain);
            let mut m = base.clone();
            if let Item::Tool { name, .. } = &mut m {
                *name = "n2".into();
            }
            assert_ne!(fp(&base), fp(&m), "name changes must re-render");
            let mut m = base.clone();
            if let Item::Tool { args_preview, .. } = &mut m {
                *args_preview = "a2".into();
            }
            assert_ne!(fp(&base), fp(&m), "args changes must re-render");
            let mut m = base.clone();
            if let Item::Tool { status, .. } = &mut m {
                *status = ToolStatus::Ok;
            }
            assert_ne!(fp(&base), fp(&m), "status changes must re-render");
            let mut m = base.clone();
            if let Item::Tool { result, .. } = &mut m {
                *result = "r2".into();
            }
            assert_ne!(fp(&base), fp(&m), "result changes must re-render");
            let mut m = base.clone();
            if let Item::Tool { error, .. } = &mut m {
                *error = "e".into();
            }
            assert_ne!(fp(&base), fp(&m), "error changes must re-render");
        });
        root.dispose();
    }

    fn thinking(iteration: u32, content: &str) -> Item {
        Item::Thinking {
            iteration,
            content: content.into(),
            reasoning: String::new(),
            call: crate::transcript::CallCost::default(),
        }
    }

    fn tool_item(key: &str, status: ToolStatus) -> Item {
        Item::Tool {
            key: key.into(),
            name: "read_file".into(),
            args_preview: "a.rs".into(),
            args_full: "path: a.rs".into(),
            status,
            result: "fn main() {}".into(),
            error: String::new(),
            sandbox: None,
        }
    }

    /// Cycle ownership is positional and stops at turn boundaries: the
    /// calls after a `Thinking` card belong to it; a user message, a steer
    /// or the final answer ends the open cycle, so a stray later call
    /// stays a plain row instead of folding into an earlier turn's panel.
    #[test]
    fn cycle_slots_group_calls_under_their_cycle_and_stop_at_turn_boundaries() {
        let items = vec![
            Item::User { text: "q".into() },
            tool_item("orphan", ToolStatus::Ok),
            thinking(1, "first"),
            tool_item("t1", ToolStatus::Ok),
            Item::Info {
                text: "note".into(),
            },
            tool_item("t2", ToolStatus::Failed),
            thinking(2, "second"),
            tool_item("t3", ToolStatus::Running),
            Item::Assistant {
                text: "answer".into(),
                final_answer: true,
            },
            tool_item("late", ToolStatus::Ok),
        ];
        let slots = cycle_slots(&items, &[], false);
        assert_eq!(slots[0], Slot::Plain);
        assert_eq!(
            slots[1],
            Slot::Plain,
            "a call before any cycle stands alone"
        );
        match &slots[2] {
            Slot::Head {
                open: false,
                tools,
                lead: false,
                ..
            } => assert_eq!(
                tools,
                &vec![
                    (ToolStatus::Ok, "read_file".into()),
                    (ToolStatus::Failed, "read_file".into())
                ]
            ),
            other => panic!("cycle 1 heads its calls: {other:?}"),
        }
        assert_eq!(slots[3], Slot::Member { open: false });
        assert_eq!(slots[4], Slot::Plain, "notices are never hidden by a fold");
        assert_eq!(slots[5], Slot::Member { open: false });
        assert_eq!(slots[7], Slot::Member { open: false });
        assert_eq!(slots[9], Slot::Plain, "the final answer closes the cycle");
        // Opening ONE cycle by key opens only that one.
        let keys = cycle_keys(&items);
        let open = vec![keys[6].clone().unwrap()];
        let slots = cycle_slots(&items, &open, false);
        assert!(matches!(slots[2], Slot::Head { open: false, .. }));
        assert!(matches!(slots[6], Slot::Head { open: true, .. }));
        assert_eq!(slots[3], Slot::Member { open: false });
        assert_eq!(slots[7], Slot::Member { open: true });
        // A folded head right after an OPEN panel takes a blank of air;
        // folded heads after folded ones pack.
        let open_first = vec![keys[2].clone().unwrap()];
        let slots = cycle_slots(&items, &open_first, false);
        assert!(matches!(
            slots[6],
            Slot::Head {
                open: false,
                lead: true,
                ..
            }
        ));
        let slots = cycle_slots(&items, &[], false);
        assert!(matches!(
            slots[6],
            Slot::Head {
                open: false,
                lead: false,
                ..
            }
        ));
        // `/details full` opens every cycle.
        let slots = cycle_slots(&items, &[], true);
        assert!(slots.iter().all(|s| !matches!(
            s,
            Slot::Head { open: false, .. } | Slot::Member { open: false }
        )));
    }

    /// A user message mid-list ends the cycle above it: a call after it
    /// stands alone (adversarial review 2026-10-10, mutant M6 survived a
    /// test whose only user message was the first item).
    #[test]
    fn a_user_message_ends_the_cycle_above_it() {
        let items = vec![
            thinking(1, "a"),
            tool_item("t1", ToolStatus::Ok),
            Item::User { text: "q2".into() },
            tool_item("t2", ToolStatus::Ok),
        ];
        let slots = cycle_slots(&items, &[], false);
        assert_eq!(slots[1], Slot::Member { open: false });
        assert_eq!(slots[3], Slot::Plain);
    }

    /// Identity is scoped to the TURN and the cycle's calls: the same
    /// `cycle 1` with the same (empty) words in two turns gets two keys,
    /// even when both turns were opened by the same user text — the
    /// first call's key carries the run (adversarial review 2026-10-10:
    /// one click opened both).
    #[test]
    fn cycle_keys_differ_across_turns_with_identical_cycles() {
        let turn = |user: &str, call: &str| {
            vec![
                Item::User { text: user.into() },
                thinking(1, ""),
                tool_item(call, ToolStatus::Ok),
                Item::Assistant {
                    text: "done".into(),
                    final_answer: true,
                },
            ]
        };
        let mut items = turn("continue", "call:runA:c1");
        items.extend(turn("continue", "call:runB:c1"));
        let keys = cycle_keys(&items);
        assert_ne!(keys[1], keys[5], "same text, same number, different runs");
        let mut items = turn("q1", "");
        items.extend(turn("q2", ""));
        let keys = cycle_keys(&items);
        assert_ne!(keys[1], keys[5], "different turns");
        // Opening turn 2's cycle opens only turn 2's.
        let slots = cycle_slots(&items, &[keys[5].clone().unwrap()], false);
        assert!(matches!(slots[1], Slot::Head { open: false, .. }));
        assert!(matches!(slots[5], Slot::Head { open: true, .. }));
        // A cycle opened BEFORE its first call landed (stored under the
        // base key) stays open when the call arrives.
        let early = vec![thinking(1, "plan")];
        let stored = cycle_keys(&early)[0].clone().unwrap();
        let mut later = early.clone();
        later.push(tool_item("call:r:c9", ToolStatus::Running));
        let slots = cycle_slots(&later, &[stored], false);
        assert!(matches!(slots[0], Slot::Head { open: true, .. }));
        assert_eq!(slots[1], Slot::Member { open: true });
    }

    /// Chips outrank the note and the stats for room: at a narrow width
    /// the awaiting-approval and failure chips stay whole.
    #[test]
    fn narrow_cycle_rows_keep_their_status_chips() {
        let t = abstracttui::theme::themes()[0].tokens;
        let header = CycleHeader {
            open: false,
            lead: false,
            trail: false,
            label: "cycle 12".into(),
            gist: "Some content".into(),
            note: Some("∴ 120 words".into()),
            chips: tool_chips(
                &t,
                &[
                    (ToolStatus::Ok, "a".into()),
                    (ToolStatus::Failed, "b".into()),
                    (ToolStatus::AwaitingApproval, "c".into()),
                ],
            ),
            stats: vec!["4s".into(), "9.8k↑ 210↓ tk".into()],
            arrow_ink: t.accent,
            label_ink: t.text,
            gist_ink: t.text_muted,
            stat_ink: t.text_muted,
            rule_ink: t.text_faint,
        };
        let segs: String = header
            .right_segments(0, false)
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        assert_eq!(segs, "3 tools ?1 ✗1 ✓1", "chips are the floor");
        let full: String = header
            .right_segments(2, true)
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        assert!(
            full.starts_with("3 tools ?1 ✗1 ✓1 · ∴ 120 words · 4s"),
            "chips lead, then the note, then stats: {full}"
        );
    }

    /// A tool-only cycle leads with its calls' names; markdown markers
    /// never reach the plain-text row.
    #[test]
    fn cycle_gist_is_plain_words_or_the_call_names() {
        assert_eq!(
            cycle_gist("read **Cargo.toml** then `cargo test`", "", &[]),
            "read Cargo.toml then cargo test"
        );
        assert_eq!(
            cycle_gist(
                "",
                "",
                &[
                    (ToolStatus::Ok, "read_file".into()),
                    (ToolStatus::Ok, "grep".into())
                ]
            ),
            "read_file, grep"
        );
        assert_eq!(cycle_gist("", "thinking aloud", &[]), "thinking aloud");
    }

    /// The folded head must repaint when a call it summarises changes
    /// status, when a call joins it, and when it opens.
    #[test]
    fn cycle_head_fingerprint_tracks_its_calls_and_open_state() {
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = crate::store::Store::create(cx);
            let head = thinking(1, "c");
            let fp = |slot: Slot| fingerprint(&head, &store, false, &slot);
            let base = fp(Slot::Head {
                open: false,
                tools: vec![(ToolStatus::Running, "x".into())],
                lead: false,
                key: String::new(),
                base: String::new(),
            });
            assert_ne!(
                base,
                fp(Slot::Head {
                    open: false,
                    tools: vec![(ToolStatus::Ok, "x".into())],
                    lead: false,
                    key: String::new(),
                    base: String::new(),
                }),
                "a call's status change repaints the chips"
            );
            assert_ne!(
                base,
                fp(Slot::Head {
                    open: false,
                    tools: vec![
                        (ToolStatus::Running, "x".into()),
                        (ToolStatus::Ok, "y".into())
                    ],
                    lead: false,
                    key: String::new(),
                    base: String::new(),
                }),
                "a new call repaints the count"
            );
            assert_ne!(
                base,
                fp(Slot::Head {
                    open: true,
                    tools: vec![(ToolStatus::Running, "x".into())],
                    lead: false,
                    key: String::new(),
                    base: String::new(),
                }),
                "opening repaints"
            );
            assert_ne!(
                base,
                fp(Slot::Head {
                    open: false,
                    tools: vec![(ToolStatus::Running, "x".into())],
                    lead: true,
                    key: String::new(),
                    base: String::new(),
                }),
                "a lead blank changes the height and repaints"
            );
        });
        root.dispose();
    }

    /// The tool chips name every status present, failures and pending
    /// approvals included, so the folded row never hides one.
    #[test]
    fn tool_chips_count_each_status() {
        let t = abstracttui::theme::themes()[0].tokens;
        let chips: Vec<String> = tool_chips(
            &t,
            &[
                (ToolStatus::Ok, "a".into()),
                (ToolStatus::Ok, "b".into()),
                (ToolStatus::Failed, "c".into()),
                (ToolStatus::AwaitingApproval, "d".into()),
            ],
        )
        .into_iter()
        .map(|(s, _)| s)
        .collect();
        assert_eq!(chips, vec!["4 tools", "?1", "✗1", "✓2"]);
        assert!(tool_chips(&t, &[]).is_empty());
    }

    /// The user card's declared height covers every row it paints: the
    /// lead blank, both pad rows, and one row per wrapped line.
    #[test]
    fn user_card_height_covers_its_rows() {
        let t = abstracttui::theme::themes()[0].tokens;
        let card = |body: &str| UserCard {
            body: body.into(),
            cap: 200,
            uncapped: false,
            icon: VoiceIcon::Idle,
            icon_ink: t.text_faint,
            icon_lit: t.accent,
            bg: user_card_bg(&t),
            bar_ink: t.accent,
            glyph_ink: t.accent,
            text_ink: t.text,
            faint_ink: t.text_muted,
        };
        assert_eq!(card("hi").height_at(80), 4);
        assert_eq!(card("a\nb\nc").height_at(80), 6);
        assert_eq!(
            card("").height_at(80),
            4,
            "an empty ask still draws its card"
        );
        let long = "word ".repeat(100);
        let c = card(&long);
        assert_eq!(c.height_at(40), 3 + c.lines_at(40).len() as i32);
        assert_ne!(
            user_card_bg(&t),
            t.bg,
            "the card ground differs from the page"
        );
    }
}
