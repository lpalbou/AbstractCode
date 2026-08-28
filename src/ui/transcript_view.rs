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
const THINKING_MAX_ROWS: usize = 10;
const TOOL_RESULT_MAX_ROWS: usize = 6;
/// Collapsed-view cap for the thinking gist: enough rows for the
/// model's actual words to lead the cycle (operator directive
/// 2026-08-19: "the first thing we should see in the turn is the
/// actual thinking"), few enough that a 30-cycle run still scans.
const THINKING_GIST_ROWS: usize = 4;

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
fn rule_block(ch: char, label: String, label_ink: Rgba, line_ink: Rgba) -> FeedBlock {
    FeedBlock::Custom(CustomBlock::new(
        |_| 2,
        move |canvas, rect| {
            if rect.w <= 0 || rect.h < 2 {
                return;
            }
            let y = rect.y + 1;
            let lead = format!("{ch}{ch} ");
            canvas.print(Point::new(rect.x, y), &lead, line_ink, Rgba::TRANSPARENT);
            let lx = rect.x + text::width(&lead);
            let fitted = text::truncate_ellipsis(&label, (rect.w - text::width(&lead) - 4).max(4));
            canvas.print(Point::new(lx, y), &fitted, label_ink, Rgba::TRANSPARENT);
            let tx = lx + text::width(&fitted) + 1;
            if rect.right() > tx {
                let tail: String = ch.to_string().repeat((rect.right() - tx) as usize);
                canvas.print(Point::new(tx, y), &tail, line_ink, Rgba::TRANSPARENT);
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
}

impl ToolRow {
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
            move |width| 1 + h_spec.error_lines(width).len() as i32,
            move |canvas, rect| {
                if rect.w <= 0 || rect.h <= 0 {
                    return;
                }
                let word_seg = format!("· {}", spec.word);
                let word_w = text::width(&word_seg);
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
                let name_avail = rect.right() - x - word_w - 1;
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
                x += word_w + 2;
                let hint_avail = rect.right() - x;
                if hint_avail >= 4 && !spec.hint.is_empty() {
                    let fitted = text::truncate_ellipsis(&spec.hint, hint_avail);
                    // Same cells and ink as a plain print; path/URL
                    // tokens additionally carry OSC-8 targets. The
                    // FITTED string is what segments, so a token the
                    // ellipsis cut simply is not a token any more.
                    crate::ui::linkify::print_linked(
                        canvas,
                        Point::new(x, rect.y),
                        &fitted,
                        spec.hint_ink,
                        spec.link_root.as_ref(),
                    );
                }
                for (i, line) in spec.error_lines(rect.w).iter().enumerate() {
                    canvas.print(
                        Point::new(rect.x + 2, rect.y + 1 + i as i32),
                        line,
                        spec.error_ink,
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
    /// One extra row AFTER the capped lines, its own ink, never
    /// capped away (the collapsed thinking card's "+reasoning …"
    /// note — ADR 0001: a hidden channel is named, never dropped).
    suffix: Option<(String, Rgba)>,
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
            suffix: None,
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

    fn suffix(mut self, s: &str, ink: Rgba) -> CappedBody {
        self.suffix = Some((s.to_string(), ink));
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
                if h_spec.suffix.is_some() {
                    h += 1;
                }
                h
            },
            move |canvas, rect| {
                let top = rect.y + i32::from(spec.lead);
                let lines = spec.lines_at(rect.w);
                for (i, line) in lines.iter().enumerate() {
                    let p = Point::new(rect.x + spec.indent, top + i as i32);
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
                if let Some((s, ink)) = &spec.suffix {
                    let fitted = text::truncate_ellipsis(s, (rect.w - spec.indent).max(4));
                    canvas.print(
                        Point::new(rect.x + spec.indent, top + lines.len() as i32),
                        &fitted,
                        *ink,
                        Rgba::TRANSPARENT,
                    );
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
    match status {
        ToolStatus::AwaitingApproval => ("?", t.warn),
        ToolStatus::Running => ("»", t.accent),
        ToolStatus::Ok => ("✓", t.ok),
        ToolStatus::Failed => ("✗", t.error),
        ToolStatus::Denied => ("⊘", t.text_muted),
        ToolStatus::Interrupted => ("◌", t.text_muted),
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
fn render_item(
    t: &TokenSet,
    item: &Item,
    store: Store,
    details: bool,
    link_root: Option<&Rc<str>>,
) -> Option<FeedItem> {
    match item {
        // TURN-weight rule (adversarial review finding 3): the user's
        // ask opens the turn and must outrank the cycle rules inside
        // it — '═' vs. their '─'.
        Item::User { text: body } => Some(
            FeedItem::new()
                .block(rule_block('═', "you".into(), t.accent, t.text_faint))
                .block(CappedBody::new(body, t.text, 200).uncapped(details).block()),
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
            let mut label = format!("cycle {iteration}");
            if let Some(ms) = call.gen_time_ms {
                label.push_str(&format!(" · {}", fmt_call_secs(ms)));
            }
            if call.input_tokens > 0 || call.output_tokens > 0 {
                label.push_str(&format!(
                    " · {}↑ {}↓ tk",
                    crate::ui::chrome::fmt_tokens(call.input_tokens),
                    crate::ui::chrome::fmt_tokens(call.output_tokens)
                ));
            }
            // Cache reuse, PROVIDER-REPORTED only (operator ask
            // 2026-08-19): how much of this prompt was served from
            // cache instead of recomputed. Unreported (0) renders
            // nothing — never the prev-input derivation dressed up as
            // a measurement (/cache owns the labeled estimate).
            if call.cached_tokens > 0 && call.input_tokens > 0 {
                let pct = (call.cached_tokens * 100 / call.input_tokens).min(100);
                label.push_str(&format!(" · {pct}% cached"));
            }
            // Label in full ink (operator report 2026-08-19: tool rows
            // out-shouted the cycle rules — the delimiter must win the
            // glance); the line stays faint so thirty rules delimit
            // without shouting.
            let mut fi = FeedItem::new().block(rule_block('─', label, t.text, t.text_faint));
            if details {
                let mut body = content.trim().to_string();
                if !reasoning.trim().is_empty() {
                    if !body.is_empty() {
                        body.push_str("\n— reasoning —\n");
                    }
                    body.push_str(reasoning.trim());
                }
                if !body.is_empty() {
                    fi = fi.block(
                        CappedBody::new(&body, t.text_muted, THINKING_MAX_ROWS * 3)
                            .uncapped(true)
                            .block(),
                    );
                }
            } else {
                let src = if content.trim().is_empty() {
                    reasoning
                } else {
                    content
                };
                // The hidden reasoning channel is NAMED as a suffix
                // row inside the same block (ADR 0001: never a silent
                // drop; a separate block would buy a rhythm blank).
                // Words, not chars (adversarial review finding 8): a
                // human gauges a hidden paragraph by words; "441 ch"
                // gauged nothing.
                let note = if !content.trim().is_empty() && !reasoning.trim().is_empty() {
                    Some(format!(
                        "… (+{} words of reasoning · /details)",
                        reasoning.split_whitespace().count()
                    ))
                } else {
                    None
                };
                if !src.trim().is_empty() {
                    let mut body = CappedBody::new(src.trim(), t.text_muted, THINKING_GIST_ROWS);
                    if let Some(n) = &note {
                        body = body.suffix(n, t.text_faint);
                    }
                    fi = fi.block(body.block());
                }
            }
            Some(fi)
        }
        Item::Tool {
            name,
            args_preview,
            args_full,
            status,
            result,
            error,
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
            if !details {
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
                        }
                        .block(),
                    ),
                );
            }
            let spans = vec![
                Span::new(format!("{glyph} "), Style::new().fg(ink)),
                Span::new(name.clone(), Style::new().fg(t.text_muted)),
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
                    CappedBody::new(args_body, t.text_faint, TOOL_RESULT_MAX_ROWS)
                        .uncapped(true)
                        .linked(link_root)
                        .block(),
                );
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
            if !result.is_empty() && *status != ToolStatus::Running {
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
                return Some(
                    FeedItem::new()
                        .block(rule_block('═', "assistant".into(), t.ok, t.text_faint))
                        .block(FeedBlock::Markdown(body.clone())),
                );
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

/// Cheap content fingerprint (FNV-1a) over everything `render_item`
/// reads, so the sync effect re-renders exactly the items that changed.
fn fingerprint(item: &Item, store: &Store) -> u64 {
    let mut h = Fnv::new();
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
            ..
        } => {
            h.byte(4);
            h.str(name);
            h.str(args_preview);
            h.body(args_full);
            h.byte(*status as u8);
            h.body(result);
            h.body(error);
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

        // ONE sync body over whichever item source the focus names.
        // Reactive property this buys: signal reads are dynamic per run,
        // so in Agent focus this effect tracks ONLY fold+focus(+details/
        // theme/images) — background convo/poller updates never wake it;
        // in Entity focus any convo write re-runs it and the fingerprint
        // fast path no-ops (trivial at chat scale).
        let sync_items = |items: &[Item]| {
            let mut st = state.borrow_mut();
            let mut rebuild = theme_id != st.theme
                || details != st.details
                || focus != st.focus
                || items.len() < st.seen.len();
            if !rebuild {
                // A mid-list visibility flip cannot be expressed with
                // keyed appends (order is push order) — rebuild instead.
                for (i, item) in items.iter().enumerate().take(st.seen.len()) {
                    let visible = is_visible(item, details);
                    if st.seen[i].1 != visible {
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
                feed.clear();
                for (i, item) in items.iter().enumerate() {
                    let fp = fingerprint(item, &store);
                    match render_item(&t, item, store, details, link_root.as_ref()) {
                        Some(fi) => {
                            feed.push(format!("i{i}"), fi);
                            st.seen.push((fp, true));
                        }
                        None => st.seen.push((fp, false)),
                    }
                }
                return;
            }
            for (i, item) in items.iter().enumerate() {
                let known = i < st.seen.len();
                let fp = fingerprint(item, &store);
                if known && st.seen[i] == (fp, is_visible(item, details)) {
                    continue; // unchanged
                }
                match render_item(&t, item, store, details, link_root.as_ref()) {
                    Some(fi) => {
                        // Existing key -> in-place replace; new key ->
                        // append (source items only ever append, so a new
                        // visible key always lands at the tail).
                        feed.push(format!("i{i}"), fi);
                        if known {
                            st.seen[i] = (fp, true);
                        } else {
                            st.seen.push((fp, true));
                        }
                    }
                    None => {
                        if known {
                            st.seen[i] = (fp, false);
                        } else {
                            st.seen.push((fp, false));
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

/// Mirror of `render_item`'s hide rules (cheap, no rendering). The sync
/// effect's ORDER correctness depends on this mirror staying exact —
/// `render_item` returning `Some` for an item this predicate calls
/// hidden would append a mid-list key at the feed tail (feed order is
/// push order). Pinned by `tests::visibility_mirror_matches_render_item`.
fn is_visible(item: &Item, details: bool) -> bool {
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
    offset: Signal<i32>,
    follow: Signal<bool>,
    empty: abstracttui::reactive::Memo<bool>,
    splash: Signal<u64>,
    anim: crate::ui::animation::FeedHandle,
    anim_frame: Signal<u64>,
) -> View {
    let tokens = *t;
    let feed = feed.clone();
    let gateway_label = ctx.gateway_label.clone();
    let workspace_root = ctx.workspace_root.clone().unwrap_or_default();
    // Right-click context menu inputs (operator ask, 2026-08-28): the
    // overlay store the engine popup opens in, and the linkify root for
    // the menu's "Copy path" row.
    let overlays = ctx.overlays.clone();
    let link_root: Option<Rc<str>> = ctx.workspace_root.as_deref().map(Rc::from);
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
            if store.restoring.get() {
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
            // gap 0: spacing lives INSIDE items (`gap_row`), so tool
            // rows pack under their cycle while sections keep air.
            let scroll = abstracttui::widgets::Scroll::new(Feed::new(&feed).gap(0).view(scx))
                .offset_y(offset)
                .follow_tail(follow)
                .layout(LayoutStyle::default().grow(1.0))
                .element(scx, &tokens)
                .build();
            // Right-click on an item opens its action menu
            // (`ui::item_menu`). A wrapper element rather than a Feed
            // hook: the engine's Feed exposes left-press only (its
            // `on_item_press`), so the wrapper hears the secondary
            // press on bubble, maps the pane row back to the item
            // through `FeedState::item_at_row` + the live scroll
            // offset, and opens the engine ContextMenu at the pointer.
            // `basis(Cells(0))` on the wrapper: the 2026-08-20 lesson —
            // an auto-sized ancestor of a Scroll re-derives a
            // content-sized basis and overflows the chrome column.
            let ui_feed = feed.clone();
            let menu_overlays = overlays.clone();
            let menu_root = link_root.clone();
            Element::new()
                .style(LayoutStyle::column().grow(1.0).basis(Dimension::Cells(0)))
                .on(abstracttui::ui::Phase::Bubble, move |ectx, ev| {
                    let abstracttui::ui::UiEvent::Mouse(m) = ev else {
                        return;
                    };
                    let abstracttui::ui::MouseKind::Down(abstracttui::ui::MouseButton::Right) =
                        m.kind
                    else {
                        return;
                    };
                    let rect = ectx.current_rect();
                    let row = m.pos.y - rect.y + offset.get_untracked();
                    let Some((key, _)) = ui_feed.item_at_row(row) else {
                        return; // a gap or past-the-end press affords nothing
                    };
                    let Some(ix) = key.strip_prefix('i').and_then(|s| s.parse::<usize>().ok())
                    else {
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
                    if actions.is_empty() {
                        return;
                    }
                    ectx.stop_propagation();
                    let act_root = menu_root.clone();
                    let _ = abstracttui::app::ContextMenu::new(actions)
                        .access_label("transcript item actions")
                        .overlays(&menu_overlays)
                        .on_action(move |k| {
                            crate::ui::item_menu::act(store, &item, k, act_root.as_deref())
                        })
                        .open(cx, m.pos);
                })
                .child(scroll)
                .build()
        },
    )
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
    let workflow = store.workflow.with(|w| {
        if w.flow_id.is_empty() {
            "none yet — /workflow picks one".to_string()
        } else {
            w.label()
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
                "diagnose:   abstractcode-tui doctor    connect: abstractcode-tui login".into(),
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
            for details in [true, false] {
                for item in &items {
                    assert_eq!(
                        is_visible(item, details),
                        render_item(&t, item, store, details, None).is_some(),
                        "visibility mirror diverged for {item:?} details={details}"
                    );
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
            };
            let fp = |i: &Item| fingerprint(i, &store);
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
}
