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
use abstracttui::render::{RichLine, Span, Style};
use abstracttui::text;
use abstracttui::widgets::{CustomBlock, Feed, FeedBlock, FeedItem, FeedState};

use crate::convo::Focus;
use crate::store::Store;
use crate::transcript::{Item, ToolStatus};

const IMAGE_ROWS: i32 = 14;
const THINKING_MAX_ROWS: usize = 10;
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
/// (rich lines have no truncate knob — first-app/0283 files it); the
/// only detail-carrying header is the tool card, whose args preview is
/// capped upstream at 200 CHARS (`value_preview(…, ARGS_PREVIEW_MAX)`
/// — chars, not cells: ~240 ASCII cells with glyph+label ⇒ 4 rows at
/// 60 cols, 3 at 80/100; a CJK-heavy preview can reach ~2× that). The
/// full preview is readable instead of cut — the accepted trade.
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

/// A header-led feed item: the rich header row first, blocks follow.
fn carded(
    glyph: &str,
    glyph_ink: Rgba,
    label: &str,
    label_ink: Rgba,
    detail: &str,
    detail_ink: Rgba,
) -> FeedItem {
    FeedItem::rich_lines(vec![header_line(
        glyph, glyph_ink, label, label_ink, detail, detail_ink,
    )])
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
}

impl CappedBody {
    fn new(body: &str, ink: Rgba, cap: usize) -> CappedBody {
        CappedBody {
            body: body.to_string(),
            ink,
            cap,
            indent: 2,
            prefix: String::new(),
        }
    }

    fn prefix(mut self, prefix: &str) -> CappedBody {
        self.prefix = prefix.into();
        self
    }

    fn no_indent(mut self) -> CappedBody {
        self.indent = 0;
        self
    }

    fn lines_at(&self, width: i32) -> Vec<String> {
        if self.body.is_empty() || self.cap == 0 {
            return Vec::new();
        }
        let (lines, _) = wrap_capped(&self.body, width - self.indent, self.cap);
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
            move |width| h_spec.lines_at(width).len() as i32,
            move |canvas, rect| {
                for (i, line) in spec.lines_at(rect.w).iter().enumerate() {
                    canvas.print(
                        Point::new(rect.x + spec.indent, rect.y + i as i32),
                        line,
                        spec.ink,
                        Rgba::TRANSPARENT,
                    );
                }
            },
        ))
    }
}

fn wrap_capped(source: &str, width: i32, cap: usize) -> (Vec<String>, usize) {
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
    let total = lines.len();
    if total > cap {
        lines.truncate(cap);
        let hidden = total - cap;
        // The count is the honest fact; no pointer at server internals
        // (operator ruling 2026-07-26). Where a fuller client view
        // exists, the card header already names it (the folded thinking
        // card's "/details full").
        lines.push(format!("… (+{hidden} more lines)"));
    }
    let n = lines.len();
    (lines, n)
}

fn tool_glyph(t: &TokenSet, status: ToolStatus) -> (&'static str, Rgba) {
    match status {
        ToolStatus::AwaitingApproval => ("?", t.warn),
        ToolStatus::Running => ("»", t.accent),
        ToolStatus::Ok => ("✓", t.ok),
        ToolStatus::Failed => ("✗", t.error),
        ToolStatus::Denied => ("⊘", t.text_muted),
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
fn render_item(t: &TokenSet, item: &Item, store: Store, details: bool) -> Option<FeedItem> {
    match item {
        Item::User { text: body } => Some(
            carded("❯", t.accent, "you", t.accent, "", t.accent)
                .block(CappedBody::new(body, t.text, 200).block()),
        ),
        Item::Steer { text: body } => Some(
            carded("↪", t.warn, "steer", t.warn, "", t.warn)
                .block(CappedBody::new(body, t.text_muted, 40).block()),
        ),
        Item::Thinking {
            iteration,
            content,
            reasoning,
            ..
        } => {
            if !details {
                return None;
            }
            // Three-state thinking (first-citizen directive: folded by
            // default, examinable on demand). FOLDED = one-line gist +
            // an honest note of what expansion holds; FULL = content
            // AND the reasoning channel as a labeled block — the old
            // render DROPPED reasoning whenever content was non-empty
            // (the defect the reasoning survey caught), which made
            // "examine them" unsatisfiable for models emitting both.
            let full = store.details_full.get();
            if !full {
                let src = if content.trim().is_empty() {
                    reasoning
                } else {
                    content
                };
                let gist = src.lines().find(|l| !l.trim().is_empty()).unwrap_or("…");
                let mut note = String::new();
                if !reasoning.trim().is_empty() && !content.trim().is_empty() {
                    note = format!(" (+reasoning {} ch)", reasoning.len());
                }
                let body = format!("{gist}{note}");
                return Some(
                    carded(
                        "∴",
                        t.text_faint,
                        &format!("cycle {iteration}"),
                        t.text_faint,
                        "folded · /details full",
                        t.text_faint,
                    )
                    .block(CappedBody::new(&body, t.text_faint, 2).block()),
                );
            }
            let mut body = content.trim().to_string();
            if !reasoning.trim().is_empty() {
                if !body.is_empty() {
                    body.push_str("\n— reasoning —\n");
                }
                body.push_str(reasoning.trim());
            }
            Some(
                carded(
                    "∴",
                    t.text_faint,
                    &format!("cycle {iteration}"),
                    t.text_faint,
                    "",
                    t.text_faint,
                )
                .block(CappedBody::new(&body, t.text_faint, THINKING_MAX_ROWS * 3).block()),
            )
        }
        Item::Tool {
            name,
            args_preview,
            status,
            result_preview,
            error,
            ..
        } => {
            // A CALLED TOOL IS ALWAYS SHOWN (operator ruling 2026-07-26:
            // "Ctrl+D should still show the tools called but no detail").
            // The clean view drops the DETAIL — the argument line and the
            // result body — never the FACT that the tool ran: a folded-
            // away trace read as "did it even do anything?". The header
            // (glyph + name + status) is the call; args + result are the
            // detail. Errors stay in both views (honesty over tidiness).
            let (glyph, ink) = tool_glyph(t, *status);
            let status_label = match status {
                ToolStatus::AwaitingApproval => " · awaiting approval",
                ToolStatus::Running => " · running",
                ToolStatus::Denied => " · denied",
                _ => "",
            };
            // Clean view: header only (+ error). The argument preview is
            // detail — dropped with the body — so a wall of finished
            // tools collapses to one scannable line each.
            let subtitle = if details { args_preview.as_str() } else { "" };
            let mut fi = carded(
                glyph,
                ink,
                &format!("{name}{status_label}"),
                t.text,
                subtitle,
                t.text_muted,
            );
            if !error.is_empty() {
                fi = fi.block(CappedBody::new(error, t.error, 3).block());
            } else if details && !result_preview.is_empty() && *status != ToolStatus::Running {
                fi = fi.block(
                    CappedBody::new(result_preview, t.text_faint, TOOL_RESULT_MAX_ROWS).block(),
                );
            }
            Some(fi)
        }
        Item::Assistant {
            text: body,
            final_answer,
        } => {
            let label = if *final_answer {
                "assistant"
            } else {
                "assistant (update)"
            };
            let ink = if *final_answer { t.ok } else { t.text_muted };
            Some(carded("✦", ink, label, ink, "", ink).block(FeedBlock::Markdown(body.clone())))
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
                        fi = fi.block(CappedBody::new(&msg, t.error, 2).block());
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
                    .block(),
            ),
        ),
        Item::Error { text: body } => Some(
            carded("✗", t.error, "error", t.error, "", t.error)
                .block(CappedBody::new(body, t.error, 12).block()),
        ),
        // Entity probe bodies (memory digests behind the always-visible
        // count chip): details-gated exactly like Thinking.
        Item::Probe { title, body } => {
            if !details {
                return None;
            }
            Some(
                carded("◈", t.text_faint, title, t.text_faint, "", t.text_faint)
                    .block(CappedBody::new(body, t.text_faint, 14).block()),
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
            h.str(text);
        }
        Item::Steer { text } => {
            h.byte(2);
            h.str(text);
        }
        Item::Thinking {
            iteration,
            content,
            reasoning,
            ..
        } => {
            h.byte(3);
            h.u64(*iteration as u64);
            h.str(content);
            h.str(reasoning);
        }
        Item::Tool {
            name,
            args_preview,
            status,
            result_preview,
            error,
            ..
        } => {
            h.byte(4);
            h.str(name);
            h.str(args_preview);
            h.byte(*status as u8);
            h.str(result_preview);
            h.str(error);
        }
        Item::Assistant { text, final_answer } => {
            h.byte(5);
            h.byte(u8::from(*final_answer));
            h.str(text);
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
            h.str(text);
        }
        Item::Error { text } => {
            h.byte(8);
            h.str(text);
        }
        Item::Probe { title, body } => {
            h.byte(9);
            h.str(title);
            h.str(body);
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
    fn str(&mut self, s: &str) {
        for b in s.as_bytes() {
            self.byte(*b);
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
pub fn wire_feed(cx: Scope, store: Store, feed: &FeedState) {
    struct SyncState {
        /// (fingerprint, visible) per source item index.
        seen: Vec<(u64, bool)>,
        details: bool,
        details_full: bool,
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
        details_full: false,
        theme: "",
        focus: Focus::Agent,
    }));
    let feed = feed.clone();
    cx.effect(move || {
        let theme = abstracttui::app::use_theme(cx).get();
        let t = theme.tokens;
        let theme_id = theme.id;
        let details = store.show_details.get();
        let details_full = store.details_full.get();
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
                || details_full != st.details_full
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
                st.details_full = details_full;
                st.focus = focus.clone();
                st.seen.clear();
                feed.clear();
                for (i, item) in items.iter().enumerate() {
                    let fp = fingerprint(item, &store);
                    match render_item(&t, item, store, details) {
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
                match render_item(&t, item, store, details) {
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
        Item::Thinking { .. } | Item::Probe { .. } => details,
        // A called tool is ALWAYS visible (operator ruling): the clean
        // view drops its detail body in `render_item`, never the card.
        Item::Tool { .. } => true,
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
/// `splash` is read ONLY inside the empty branch (conditional signal
/// tracking): once conversation starts, the pane carries no frame
/// dependency and animation ticks can never remount the Scroll.
#[allow(clippy::too_many_arguments)]
pub fn pane(
    _cx: Scope,
    t: &TokenSet,
    store: Store,
    ctx: &crate::ui::UiCtx,
    feed: &FeedState,
    offset: Signal<i32>,
    follow: Signal<bool>,
    empty: abstracttui::reactive::Memo<bool>,
    splash: Signal<u64>,
) -> View {
    let tokens = *t;
    let feed = feed.clone();
    let gateway_label = ctx.gateway_label.clone();
    let workspace_root = ctx.workspace_root.clone().unwrap_or_default();
    dyn_view_scoped(
        LayoutStyle::column().grow(1.0).padding(Edges::hv(1, 0)),
        move |scx| {
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
            abstracttui::widgets::Scroll::new(Feed::new(&feed).gap(1).view(scx))
                .offset_y(offset)
                .follow_tail(follow)
                .layout(LayoutStyle::default().grow(1.0))
                .element(scx, &tokens)
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
    let muted = t.text_muted;
    let faint = t.text_faint;
    let text_ink = t.text;
    let error = t.error;
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
    col = col.child(crate::ui::logo::logo(
        t,
        frame,
        &format!("{} · rendered by AbstractTUI", crate::cli::VERSION),
    ));
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
                status,
                result_preview: "out".into(),
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
                        render_item(&t, item, store, details).is_some(),
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
        // unbounded count (cap + 1 overflow line at most).
        for width in [-5, 0, 1, 2, 4, 10, 80] {
            let (lines, n) = wrap_capped("héllo wörld — ééé 漢字テスト line\nsecond", width, 3);
            assert_eq!(lines.len(), n);
            assert!(
                n <= 4,
                "cap 3 + one overflow line, got {n} at width {width}"
            );
        }
        // Cap overflow appends exactly one marker line.
        let long = (0..40)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (lines, n) = wrap_capped(&long, 20, 6);
        assert_eq!(n, 7);
        assert!(lines.last().unwrap().contains("more lines"));
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
                status: ToolStatus::Running,
                result_preview: "r".into(),
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
            if let Item::Tool { result_preview, .. } = &mut m {
                *result_preview = "r2".into();
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
