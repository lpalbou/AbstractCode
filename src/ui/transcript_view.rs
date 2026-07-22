//! Transcript pane: fold items projected into a `widgets::Feed`.
//!
//! The engine owns the hard parts since 0.2.0 — keyed items, windowed
//! paint, measured content extent, follow-tail — so this module is a
//! PROJECTION: `Item` → `FeedItem` (custom-drawn cards for colored
//! chrome, real markdown blocks for assistant bodies, mosaic cells for
//! images), plus a sync effect that keeps the feed matched to the fold.
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
use abstracttui::text;
use abstracttui::widgets::{CustomBlock, Feed, FeedBlock, FeedItem, FeedState};

use crate::store::Store;
use crate::transcript::{Item, ToolStatus};

const IMAGE_ROWS: i32 = 14;
const THINKING_MAX_ROWS: usize = 10;
const TOOL_RESULT_MAX_ROWS: usize = 6;

// ---------------------------------------------------------------------------
// Card rendering (custom blocks: colored chrome the theme-ink Text
// block cannot express)
// ---------------------------------------------------------------------------

/// A one-item card: optional glyph header + wrapped body lines, each
/// with its own ink. Wraps at draw width (the height callback and the
/// draw share `lines_at`, so the geometry is honest by construction).
struct Card {
    glyph: String,
    glyph_ink: Rgba,
    label: String,
    label_ink: Rgba,
    detail: String,
    detail_ink: Rgba,
    body: String,
    body_ink: Rgba,
    body_cap: usize,
    body_indent: i32,
    /// Prefix every body line (the `· ` of info items).
    body_prefix: String,
}

impl Card {
    fn header(glyph: &str, glyph_ink: Rgba, label: String, label_ink: Rgba) -> Card {
        Card {
            glyph: glyph.into(),
            glyph_ink,
            label,
            label_ink,
            detail: String::new(),
            detail_ink: glyph_ink,
            body: String::new(),
            body_ink: glyph_ink,
            body_cap: 0,
            body_indent: 2,
            body_prefix: String::new(),
        }
    }

    fn detail(mut self, detail: String, ink: Rgba) -> Card {
        self.detail = detail;
        self.detail_ink = ink;
        self
    }

    fn body(mut self, body: &str, ink: Rgba, cap: usize) -> Card {
        self.body = body.to_string();
        self.body_ink = ink;
        self.body_cap = cap;
        self
    }

    fn body_prefix(mut self, prefix: &str) -> Card {
        self.body_prefix = prefix.into();
        self
    }

    fn no_indent(mut self) -> Card {
        self.body_indent = 0;
        self
    }

    fn has_header(&self) -> bool {
        !self.glyph.is_empty() || !self.label.is_empty()
    }

    fn lines_at(&self, width: i32) -> Vec<String> {
        if self.body.is_empty() || self.body_cap == 0 {
            return Vec::new();
        }
        let (lines, _) = wrap_capped(&self.body, width - self.body_indent, self.body_cap);
        if self.body_prefix.is_empty() {
            lines
        } else {
            // Prefix the FIRST line only; continuations hang-indent under
            // it. Per-line bullets made one wrapped notice read as
            // several separate ones (live review, 2026-07-22).
            let hang = " ".repeat(text::width(&self.body_prefix).max(0) as usize);
            lines
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    if i == 0 {
                        format!("{}{l}", self.body_prefix)
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
                let header = i32::from(h_spec.has_header());
                header + h_spec.lines_at(width).len() as i32
            },
            move |canvas, rect| {
                let mut y = rect.y;
                if spec.has_header() {
                    let mut x = rect.x;
                    if !spec.glyph.is_empty() {
                        x += canvas.print(
                            Point::new(x, y),
                            &spec.glyph,
                            spec.glyph_ink,
                            Rgba::TRANSPARENT,
                        );
                        x += canvas.print(Point::new(x, y), " ", spec.glyph_ink, Rgba::TRANSPARENT);
                    }
                    x += canvas.print(
                        Point::new(x, y),
                        &spec.label,
                        spec.label_ink,
                        Rgba::TRANSPARENT,
                    );
                    if !spec.detail.is_empty() {
                        let avail = (rect.right() - x - 1).max(0);
                        let fitted = text::truncate_ellipsis(&format!("  {}", spec.detail), avail);
                        canvas.print(
                            Point::new(x, y),
                            &fitted,
                            spec.detail_ink,
                            Rgba::TRANSPARENT,
                        );
                    }
                    y += 1;
                }
                for line in spec.lines_at(rect.w) {
                    canvas.print(
                        Point::new(rect.x + spec.body_indent, y),
                        &line,
                        spec.body_ink,
                        Rgba::TRANSPARENT,
                    );
                    y += 1;
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
        lines.push(format!(
            "… (+{hidden} more lines — full text in the run ledger)"
        ));
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
            let mut caps = abstracttui::term::Capabilities::default();
            caps.unicode_ok = true;
            caps.truecolor = true;
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
            FeedItem::new().block(
                Card::header("❯", t.accent, "you".into(), t.accent)
                    .body(body, t.text, 200)
                    .block(),
            ),
        ),
        Item::Steer { text: body } => Some(
            FeedItem::new().block(
                Card::header("↪", t.warn, "steer".into(), t.warn)
                    .body(body, t.text_muted, 40)
                    .block(),
            ),
        ),
        Item::Thinking {
            iteration,
            content,
            reasoning,
        } => {
            if !details {
                return None;
            }
            let body = if content.trim().is_empty() {
                reasoning
            } else {
                content
            };
            Some(
                FeedItem::new().block(
                    Card::header(
                        "∴",
                        t.text_faint,
                        format!("cycle {iteration}"),
                        t.text_faint,
                    )
                    .body(body, t.text_faint, THINKING_MAX_ROWS)
                    .block(),
                ),
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
            // Clean view folds FINISHED tool cards entirely. Active work
            // (awaiting/running), failures, and denials stay visible in
            // both views — a pending approval or an error must never
            // hide behind a toggle.
            if !details && *status == ToolStatus::Ok && error.is_empty() {
                return None;
            }
            let (glyph, ink) = tool_glyph(t, *status);
            let status_label = match status {
                ToolStatus::AwaitingApproval => " · awaiting approval",
                ToolStatus::Running => " · running",
                ToolStatus::Denied => " · denied",
                _ => "",
            };
            let mut card = Card::header(glyph, ink, format!("{name}{status_label}"), t.text)
                .detail(args_preview.clone(), t.text_muted);
            if !error.is_empty() {
                // Errors stay visible in both views (honesty over tidiness).
                card = card.body(error, t.error, 3);
            } else if details && !result_preview.is_empty() && *status != ToolStatus::Running {
                card = card.body(result_preview, t.text_faint, TOOL_RESULT_MAX_ROWS);
            }
            Some(FeedItem::new().block(card.block()))
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
            Some(
                FeedItem::new()
                    .block(Card::header("✦", ink, label.into(), ink).block())
                    .block(FeedBlock::Markdown(body.clone())),
            )
        }
        Item::Image {
            artifact_id, label, ..
        } => {
            let mut fi = FeedItem::new()
                .block(Card::header("▦", t.accent, label.clone(), t.text_muted).block());
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
                        fi = fi.block(
                            Card::header("", t.error, String::new(), t.error)
                                .body(&msg, t.error, 2)
                                .block(),
                        );
                    }
                }
                None => {
                    fi = fi.block(
                        Card::header("", t.text_faint, String::new(), t.text_faint)
                            .body("fetching image…", t.text_faint, 1)
                            .block(),
                    );
                }
            }
            Some(fi)
        }
        Item::Info { text: body } => Some(
            FeedItem::new().block(
                Card::header("", t.text_faint, String::new(), t.text_faint)
                    .body(body, t.text_faint, 6)
                    .body_prefix("· ")
                    .no_indent()
                    .block(),
            ),
        ),
        Item::Error { text: body } => Some(
            FeedItem::new().block(
                Card::header("✗", t.error, "error".into(), t.error)
                    .body(body, t.error, 12)
                    .block(),
            ),
        ),
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
        /// (fingerprint, visible) per fold item index.
        seen: Vec<(u64, bool)>,
        details: bool,
        theme: &'static str,
    }
    let state = Rc::new(RefCell::new(SyncState {
        seen: Vec::new(),
        details: true,
        theme: "",
    }));
    let feed = feed.clone();
    cx.effect(move || {
        let theme = abstracttui::app::use_theme(cx).get();
        let t = theme.tokens;
        let theme_id = theme.id;
        let details = store.show_details.get();
        // Image loads re-render image items (fingerprints read entries).
        let _ = store.images.with(|v| v.len());

        store.fold.with(|f| {
            let mut st = state.borrow_mut();
            let mut rebuild =
                theme_id != st.theme || details != st.details || f.items.len() < st.seen.len();
            if !rebuild {
                // A mid-list visibility flip cannot be expressed with
                // keyed appends (order is push order) — rebuild instead.
                for (i, item) in f.items.iter().enumerate().take(st.seen.len()) {
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
                st.seen.clear();
                feed.clear();
                for (i, item) in f.items.iter().enumerate() {
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
            for (i, item) in f.items.iter().enumerate() {
                let known = i < st.seen.len();
                let fp = fingerprint(item, &store);
                if known && st.seen[i] == (fp, is_visible(item, details)) {
                    continue; // unchanged
                }
                match render_item(&t, item, store, details) {
                    Some(fi) => {
                        // Existing key -> in-place replace; new key ->
                        // append (fold items only ever append, so a new
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
        });
    });
}

/// Mirror of `render_item`'s hide rules (cheap, no rendering). The sync
/// effect's ORDER correctness depends on this mirror staying exact —
/// `render_item` returning `Some` for an item this predicate calls
/// hidden would append a mid-list key at the feed tail (feed order is
/// push order). Pinned by `tests::visibility_mirror_matches_render_item`.
fn is_visible(item: &Item, details: bool) -> bool {
    match item {
        Item::Thinking { .. } => details,
        Item::Tool { status, error, .. } => {
            details || *status != ToolStatus::Ok || !error.is_empty()
        }
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// The pane
// ---------------------------------------------------------------------------

/// The transcript pane: `Scroll` over the feed, engine follow-tail.
/// Rebuilt only when the empty/connection state flips (memoized) or the
/// theme changes — item traffic never remounts the scroll.
///
/// "Empty" = no CONVERSATION yet: boot pushes Info notices (session id,
/// workspace policy) into the fold, and gating on `items.is_empty()`
/// made the guidance unreachable on every normal launch (adversary P2,
/// 2026-07-22). Info-only folds show the guidance WITH the notices
/// below it — never instead of them.
pub fn pane(
    cx: Scope,
    t: &TokenSet,
    store: Store,
    feed: &FeedState,
    offset: Signal<i32>,
    follow: Signal<bool>,
) -> View {
    let tokens = *t;
    let feed = feed.clone();
    let empty = cx.memo(move || {
        store
            .fold
            .with(|f| f.items.iter().all(|i| matches!(i, Item::Info { .. })))
    });
    dyn_view_scoped(
        LayoutStyle::column().grow(1.0).padding(Edges::hv(1, 0)),
        move |scx| {
            if empty.get() {
                let conn = store.conn.get();
                // Read the fold ONLY on this branch: while the guidance
                // shows, new notices re-render it; once conversation
                // starts, the pane carries no fold dependency at all.
                let notices: Vec<String> = store.fold.with(|f| {
                    f.items
                        .iter()
                        .filter_map(|i| match i {
                            Item::Info { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect()
                });
                return empty_state(&tokens, &conn, &notices);
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

fn empty_state(t: &TokenSet, conn: &crate::store::Conn, notices: &[String]) -> View {
    let muted = t.text_muted;
    let faint = t.text_faint;
    let accent = t.accent;
    let error = t.error;
    let line = |s: String, ink: Rgba| {
        Element::new()
            .style(LayoutStyle::line(1))
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
    let mut col = Element::new().style(
        LayoutStyle::column()
            .grow(1.0)
            .justify(Justify::Center)
            .gap(1),
    );
    col = col.child(line("▲ AbstractCode".into(), accent));
    if let crate::store::Conn::Down(msg) = conn {
        // A dead connection must teach RECOVERY, not "describe a task".
        col = col
            .child(line(
                format!(
                    "gateway unreachable — {}",
                    abstracttui::text::truncate_ellipsis(msg, 70)
                ),
                error,
            ))
            .child(line("start one:  abstractgateway serve".into(), muted))
            .child(line(
                "diagnose:   abstractcode-tui doctor    connect: abstractcode-tui login".into(),
                muted,
            ))
            .child(line(
                "the app reconnects automatically once the gateway answers".into(),
                faint,
            ));
        return col.build();
    }
    col = col
        .child(line(
            "describe a task below — the agent runs durably on the gateway".into(),
            muted,
        ))
        .child(line(
            "/help commands · /workflow agents · /model providers · /theme looks".into(),
            faint,
        ))
        .child(line("rendered by AbstractTUI".into(), faint));
    // Boot notices (session id, workspace policy) stay visible under the
    // guidance — dim, one centered line each, honesty kept.
    for n in notices {
        col = col.child(line(format!("· {n}"), faint));
    }
    col.build()
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
