//! The attachment preview modal: text documents and pictures, drawn
//! from the real bytes on disk.
//!
//! The engine draws images (`gfx::decode_image` + the mosaic ladder),
//! so a staged attachment does not have to be a filename taken on
//! trust. `crate::preview` is the loader (worker thread, pure); this is
//! the surface: a header that names what the file IS, a scrollable body,
//! and a hint row that never hides a caveat (a cut text file SAYS it was
//! cut; a format the engine cannot draw is NAMED, with the reminder that
//! attaching it still works).
//!
//! Reactive by construction: the modal opens on `PreviewBody::Loading`
//! and the loader's answer arrives later through `store.preview`, so a
//! slow decode paints "reading…" instead of freezing the frame.

use std::rc::Rc;
use std::sync::Arc;

use abstracttui::prelude::*;
use abstracttui::text;

use crate::preview::{PreviewBody, PreviewState};
use crate::runner::Cmd;
use crate::store::Store;
use crate::ui::UiCtx;

/// One drawable text row: a wrapped slice plus the source line number
/// on the row that STARTS that line (continuations keep an empty
/// gutter — the numbers stay a true index into the file).
#[derive(Clone, PartialEq, Eq)]
struct Row {
    num: Option<usize>,
    text: String,
}

/// The wrapped document plus the gutter it was wrapped against. Both
/// travel together so the DRAW can never disagree with the WRAP about
/// how many columns the text had — a disagreement there is a silent
/// horizontal cut.
#[derive(PartialEq, Eq)]
struct Doc {
    gutter: i32,
    rows: Vec<Row>,
}

/// Gutter width for a document of `lines` lines: the widest number
/// plus one space. Fixed-width gutters truncate their own numbers on
/// long files (`119983` in five columns is `1199…`), and the gutter is
/// supposed to be a TRUE index into the file.
fn gutter_for(lines: usize) -> i32 {
    (lines.max(1).to_string().len() as i32 + 1).clamp(2, 12)
}

/// What the body draws this frame — cheap to clone (the bitmap is an
/// Arc, the rows come from a memo), so the reactive read never copies a
/// 512 KB document. The text case carries NO payload on purpose: its
/// facts (line count, the cut, the lossy decode) belong to the header,
/// which reads them straight off the body.
#[derive(Clone)]
enum Kind {
    Loading,
    Text,
    Image(Arc<abstracttui::widgets::Bitmap>),
    Unavailable(String),
}

fn kind_of(state: &PreviewState) -> Kind {
    match &state.body {
        PreviewBody::Loading => Kind::Loading,
        PreviewBody::Text(_) => Kind::Text,
        PreviewBody::Image(i) => Kind::Image(i.bitmap.clone()),
        PreviewBody::Unavailable { reason } => Kind::Unavailable(reason.clone()),
    }
}

/// Preview one PENDING chip by POSITION (the manager's `p`/Enter,
/// where the cursor is an index into the live list).
pub(crate) fn open_pending(cx: Scope, store: Store, ctx: &UiCtx, index: usize) {
    let chip = store
        .pending_attachments
        .with_untracked(|p| p.get(index).cloned());
    let Some(chip) = chip else {
        store.notify("nothing staged to preview");
        return;
    };
    open_state(cx, store, ctx, chip.path, chip.name, chip.size);
}

/// Preview one PENDING chip by its CANONICAL path — what the clickable
/// chips row uses. A click carries the identity of the file the user
/// pointed at, never a position that another lane may have shifted
/// (chips can be removed, and a send clears the batch, between the
/// frame that drew the row and the frame that delivers the release).
pub(crate) fn open_chip(cx: Scope, store: Store, ctx: &UiCtx, path: &str) {
    let chip = store
        .pending_attachments
        .with_untracked(|p| p.iter().find(|a| a.path == path).cloned());
    let Some(chip) = chip else {
        store.notify("that attachment is no longer staged");
        return;
    };
    open_state(cx, store, ctx, chip.path, chip.name, chip.size);
}

/// Preview any local path (`/attach preview <path>` — works on a file
/// that is not staged at all, which is the point: look before you
/// attach). Refusals notify with the reason; nothing opens.
pub fn open_path(cx: Scope, store: Store, ctx: &UiCtx, raw: &str) {
    let expanded = crate::paths::expand_path_spelling(raw);
    let Ok(canon) = std::fs::canonicalize(&expanded) else {
        store.notify(format!("no such file: {expanded}"));
        return;
    };
    let Ok(meta) = std::fs::metadata(&canon) else {
        store.notify(format!("no such file: {expanded}"));
        return;
    };
    if meta.is_dir() {
        store.notify(format!(
            "{expanded} is a directory — preview takes one file"
        ));
        return;
    }
    let path = canon.display().to_string();
    let name = canon
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());
    open_state(cx, store, ctx, path, name, meta.len());
}

/// Mint the loading state, dispatch the load, open the modal. The seq
/// mint is the staleness guard: `runner::apply_preview` drops any body
/// that no longer names the open preview.
fn open_state(cx: Scope, store: Store, ctx: &UiCtx, path: String, name: String, size: u64) {
    let seq = store.preview_seq.get_untracked().wrapping_add(1);
    store.preview_seq.set(seq);
    store
        .preview
        .set(Some(PreviewState::loading(seq, path.clone(), name, size)));
    if !ctx.send(Cmd::LoadPreview { seq, path }) {
        // The worker is DEAD (its panic already notified): say so here
        // rather than leave "reading…" spinning forever.
        crate::runner::apply_preview(
            &store,
            seq,
            PreviewBody::Unavailable {
                reason: "the client's worker thread is not running — restart to preview".into(),
            },
        );
    }
    open_modal(cx, store, ctx, seq);
}

/// Closing is just closing: the modal scope's cleanup drops the body
/// (see `open_modal`), so EVERY close path — Esc, another modal
/// replacing this one, quit — frees it, not only the one that goes
/// through here.
fn close(ctx: &UiCtx) {
    ctx.close_modal();
}

/// Rows the body scrolls: every logical line wrapped to `width`, the
/// first row of each carrying its line number. Built ONCE per body
/// (a memo keyed on `store.preview`), never per keystroke.
fn build_rows(lines: &[String], width: i32) -> Vec<Row> {
    let width = width.max(1);
    let mut rows = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        // `text::wrap` yields one row for an empty line, so blank lines
        // in the document survive as blank rows.
        for (seg_ix, seg) in text::wrap(line, width).into_iter().enumerate() {
            rows.push(Row {
                num: (seg_ix == 0).then_some(i + 1),
                text: seg,
            });
        }
    }
    rows
}

/// Header: what this file IS, in the order an operator asks it —
/// name, size, kind, and the caveat when there is one.
fn header_line(state: &PreviewState) -> String {
    // The SIZE comes from the loader's own stat when it has one: the
    // chip's size was measured at attach time and a growing log would
    // otherwise print two different sizes on one line.
    let size = match &state.body {
        PreviewBody::Text(t) => t.total_bytes,
        _ => state.size,
    };
    let base = format!("{} · {}", state.name, crate::paths::human_size(size));
    match &state.body {
        PreviewBody::Loading => format!("{base} · reading…"),
        PreviewBody::Text(t) => {
            let kind = if t.utf16 { "UTF-16 text" } else { "text" };
            let mut s = format!("{base} · {kind} · {} lines", t.lines.len());
            if t.truncated {
                s.push_str(&format!(
                    " · showing the first {} of {}",
                    crate::paths::human_size(t.shown_bytes),
                    crate::paths::human_size(t.total_bytes)
                ));
            }
            if t.lossy {
                s.push_str(" · invalid encoding shown as \u{fffd}");
            }
            if t.ansi_stripped {
                s.push_str(" · ANSI color codes hidden");
            }
            s
        }
        PreviewBody::Image(i) => {
            format!("{base} · {} {}×{}", i.format, i.source_px.0, i.source_px.1)
        }
        PreviewBody::Unavailable { .. } => format!("{base} · no preview"),
    }
}

/// Open the modal over whatever `store.preview` currently holds.
///
/// `seq` identifies the preview this modal belongs to: the scope
/// cleanup (which fires on EVERY close — Esc, quit, another modal
/// taking the slot) drops the body only while that seq is still the
/// open one, so a preview opened over a preview is never wiped by the
/// outgoing modal's cleanup.
fn open_modal(cx: Scope, store: Store, ctx: &UiCtx, seq: u64) {
    let ctx2 = ctx.clone();
    let vp = abstracttui::app::current_viewport();
    // As big as the terminal comfortably allows: a preview that shows
    // six lines of a document is not a preview. Never WIDER or TALLER
    // than the terminal, though — the engine clamps an oversized
    // request to the viewport (`modal_bounds`), and a request the panel
    // cannot honor makes every width computed from it a lie.
    let size = Size::new(
        if vp.w <= 40 {
            vp.w.max(1)
        } else {
            100.min(vp.w - 4)
        },
        if vp.h <= 14 {
            vp.h.max(1)
        } else {
            (vp.h - 4).min(40)
        },
    );
    ctx.open_modal(cx, size, move |mcx| {
        // Free the body when this modal dies, whatever kills it. A
        // preview left resident behind an unrelated modal retains its
        // decoded bitmap (or 512 KB of lines) with no reader.
        mcx.on_cleanup(move || {
            // Teardown order is not ours to assume: at app exit the
            // store's scope can be disposed before this cleanup runs,
            // and a disposed READ panics. "Gone" is a valid answer here
            // — there is nothing left to free.
            if !store.preview.is_alive() {
                return;
            }
            let mine = store
                .preview
                .with_untracked(|p| p.as_ref().is_some_and(|s| s.seq == seq));
            if mine {
                store.preview.set(None);
            }
        });
        let top = mcx.signal(0usize);
        // The panel is `size` clamped to the LIVE viewport, and the
        // engine re-clamps it on every resize — so both the wrap width
        // and the page height must be read from the viewport, not from
        // the request. Wrapping against a stale width is a silent
        // horizontal cut: `text_view` would ellipsize every row and
        // nothing would say so.
        let vp_sig = abstracttui::app::use_viewport(mcx);
        // Panel padding is 1 on each side (engine); this column adds
        // none, so a narrow terminal spends its columns on the file.
        let content_w = move |vp: Size| (size.w.min(vp.w) - 2).max(4);
        // Body rows = panel height − padding(2) − title(1) − hint(1) −
        // the two gaps. Used for paging and the scroll clamp; the draw
        // always uses the true rect.
        let body_h = move || (size.h.min(vp_sig.get_untracked().h) - 6).max(1) as usize;
        // Wrapped rows are derived from the body and the width alone,
        // so scrolling never re-wraps the document (a 512 KB file would
        // otherwise re-wrap on every arrow key).
        let doc: abstracttui::reactive::Memo<Rc<Doc>> = mcx.memo(move || {
            let avail = content_w(vp_sig.get());
            // ONE borrow of the signal: the gutter depends on the line
            // count, so it is computed inside the same `with`.
            Rc::new(store.preview.with(|p| match p.as_ref().map(|s| &s.body) {
                Some(PreviewBody::Text(t)) => {
                    let gutter = gutter_for(t.lines.len());
                    Doc {
                        gutter,
                        rows: build_rows(&t.lines, (avail - gutter).max(4)),
                    }
                }
                _ => Doc {
                    gutter: 2,
                    rows: Vec::new(),
                },
            }))
        });
        let max_top = move || {
            doc.with_untracked(|d| d.rows.len())
                .saturating_sub(body_h())
        };
        let scroll_by = move |delta: i64| {
            top.update(|v| {
                *v = (*v as i64 + delta).clamp(0, max_top() as i64) as usize;
            });
        };
        Element::new()
            .style(LayoutStyle::column().gap(1))
            .focusable()
            .autofocus()
            .shortcut(KeyChord::plain(Key::Escape), {
                let ctx = ctx2.clone();
                move |_| close(&ctx)
            })
            .shortcut(KeyChord::plain(Key::Enter), {
                let ctx = ctx2.clone();
                move |_| close(&ctx)
            })
            .shortcut(KeyChord::plain(Key::Up), move |_| scroll_by(-1))
            .shortcut(KeyChord::plain(Key::Down), move |_| scroll_by(1))
            .shortcut(KeyChord::plain(Key::PageUp), move |_| {
                scroll_by(-(body_h() as i64))
            })
            .shortcut(KeyChord::plain(Key::PageDown), move |_| {
                scroll_by(body_h() as i64)
            })
            .shortcut(KeyChord::plain(Key::Home), move |_| top.set(0))
            .shortcut(KeyChord::plain(Key::End), move |_| top.set(max_top()))
            .child(dyn_view(LayoutStyle::line(1).shrink(0.0), move || {
                let t2 = abstracttui::app::current_theme().tokens;
                let title = store
                    .preview
                    .with(|p| p.as_ref().map(header_line))
                    .unwrap_or_else(|| "preview".into());
                Element::new()
                    .style(LayoutStyle::line(1).shrink(0.0))
                    .draw(move |canvas, rect| {
                        let fitted = text::truncate_ellipsis(&title, rect.w.max(4));
                        canvas.print(
                            Point::new(rect.x, rect.y),
                            &fitted,
                            t2.accent,
                            Rgba::TRANSPARENT,
                        );
                    })
                    .build()
            }))
            .child(dyn_view(
                LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),
                move || {
                    let t2 = abstracttui::app::current_theme().tokens;
                    let kind = store.preview.with(|p| p.as_ref().map(kind_of));
                    match kind {
                        None => Element::new().build(),
                        Some(Kind::Loading) => note_view("reading the file…", t2.text_faint),
                        Some(Kind::Unavailable(reason)) => note_view(&reason, t2.text_muted),
                        Some(Kind::Image(bitmap)) => image_view(bitmap),
                        Some(Kind::Text) => text_view(doc.get(), top.get(), t2),
                    }
                },
            ))
            .child(dyn_view(LayoutStyle::line(1).shrink(0.0), move || {
                let t2 = abstracttui::app::current_theme().tokens;
                let is_text = store
                    .preview
                    .with(|p| matches!(p.as_ref().map(|s| &s.body), Some(PreviewBody::Text(_))));
                let hint = if is_text {
                    let n = doc.with(|d| d.rows.len());
                    let first = (top.get() + 1).min(n.max(1));
                    format!("row {first}/{n} · ↑↓ PgUp/PgDn Home/End scroll · Esc closes")
                } else {
                    "Esc closes".to_string()
                };
                Element::new()
                    .style(LayoutStyle::line(1).shrink(0.0))
                    .draw(move |canvas, rect| {
                        let fitted = text::truncate_ellipsis(&hint, rect.w.max(4));
                        canvas.print(
                            Point::new(rect.x, rect.y),
                            &fitted,
                            t2.text_faint,
                            Rgba::TRANSPARENT,
                        );
                    })
                    .build()
            }))
            .build()
    });
}

/// A wrapped one-off note (loading, or the honest refusal).
fn note_view(msg: &str, ink: Rgba) -> View {
    let msg = msg.to_string();
    Element::new()
        .style(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)))
        .draw(move |canvas, rect| {
            for (i, line) in text::wrap(&msg, (rect.w - 1).max(8))
                .into_iter()
                .enumerate()
            {
                let y = rect.y + i as i32;
                if y >= rect.bottom() {
                    break;
                }
                canvas.print(Point::new(rect.x, y), &line, ink, Rgba::TRANSPARENT);
            }
        })
        .build()
}

/// The scrolled document window. Only the visible rows are touched per
/// frame — the Rc keeps the document itself out of the draw closure's
/// clone path.
fn text_view(doc: Rc<Doc>, top: usize, t: TokenSet) -> View {
    let (ink, faint) = (t.text, t.text_faint);
    let gutter = doc.gutter;
    Element::new()
        .style(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)))
        .draw(move |canvas, rect| {
            let h = rect.h.max(0) as usize;
            for (i, row) in doc.rows.iter().skip(top).take(h).enumerate() {
                let y = rect.y + i as i32;
                if y >= rect.bottom() {
                    break;
                }
                if let Some(n) = row.num {
                    let label = format!("{n:>width$}", width = (gutter - 1).max(1) as usize);
                    let fitted = text::truncate_ellipsis(&label, gutter - 1);
                    canvas.print(Point::new(rect.x, y), &fitted, faint, Rgba::TRANSPARENT);
                }
                let avail = (rect.right() - rect.x - gutter).max(0);
                let fitted = text::truncate_ellipsis(&row.text, avail);
                canvas.print(
                    Point::new(rect.x + gutter, y),
                    &fitted,
                    ink,
                    Rgba::TRANSPARENT,
                );
            }
        })
        .build()
}

/// The picture, contain-fit into the body rect. Same ladder as the
/// in-feed image block (`ui::transcript_view::image_block`) — half
/// blocks / quadrants / sextants / braille, chosen from LIVE caps at
/// draw time so a probe upgrade mid-session re-renders honestly.
fn image_view(bitmap: Arc<abstracttui::widgets::Bitmap>) -> View {
    Element::new()
        .style(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)))
        .draw(move |canvas, rect| {
            let (bw, bh) = (bitmap.width().max(1) as f64, bitmap.height().max(1) as f64);
            // A cell is ~1 wide x 2 tall, so displayed aspect is
            // cols / (2*rows) — fit height first, then clamp on width.
            let aspect = bw / bh;
            let mut rows = rect.h.max(1);
            let mut cols = ((2.0 * aspect * rows as f64).round() as i32).max(1);
            if cols > rect.w.max(1) {
                cols = rect.w.max(1);
                rows = ((cols as f64 / (2.0 * aspect)).round() as i32).clamp(1, rows);
            }
            let caps = abstracttui::app::current_caps();
            let target = Rect::new(rect.x, rect.y, cols, rows);
            for patch in abstracttui::gfx::mosaic::render_to_cells(&bitmap, target, &caps) {
                canvas.put(patch.pos, patch.ch, patch.fg, patch.bg);
            }
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_number_logical_lines_and_keep_wraps_unnumbered() {
        let lines = vec![
            "short".to_string(),
            "a fairly long line that must wrap at least once".to_string(),
            String::new(),
            "tail".to_string(),
        ];
        let rows = build_rows(&lines, 20);
        assert_eq!(rows[0].num, Some(1));
        assert_eq!(rows[0].text, "short");
        assert_eq!(rows[1].num, Some(2));
        // The wrapped tail of line 2 carries no number.
        assert!(rows[2].num.is_none());
        // Every logical line survives, blank ones included.
        let numbered: Vec<usize> = rows.iter().filter_map(|r| r.num).collect();
        assert_eq!(numbered, vec![1, 2, 3, 4]);
        assert_eq!(rows.iter().find(|r| r.num == Some(4)).unwrap().text, "tail");
    }

    #[test]
    fn the_gutter_grows_with_the_line_count() {
        // A fixed-width gutter truncates its own numbers on long files
        // ("119983" in five columns is "1199…") — the gutter is meant
        // to be a true index into the file.
        assert_eq!(gutter_for(0), 2);
        assert_eq!(gutter_for(9), 2);
        assert_eq!(gutter_for(10), 3);
        assert_eq!(gutter_for(120_000), 7);
        let rows = build_rows(&vec!["x".to_string(); 120_000], 40);
        let label = format!(
            "{:>width$}",
            120_000,
            width = (gutter_for(120_000) - 1) as usize
        );
        assert_eq!(
            label.len(),
            6,
            "the number fits the gutter it was sized for"
        );
        assert_eq!(rows.last().unwrap().num, Some(120_000));
    }

    #[test]
    fn header_states_the_cut_and_the_lossy_decode() {
        let state = PreviewState {
            seq: 1,
            path: "/x/big.log".into(),
            name: "big.log".into(),
            size: 3 * 1024 * 1024,
            body: PreviewBody::Text(crate::preview::TextPreview {
                lines: vec!["a".into(), "b".into()],
                shown_bytes: 512 * 1024,
                total_bytes: 3 * 1024 * 1024,
                truncated: true,
                lossy: true,
                ansi_stripped: false,
                utf16: false,
            }),
        };
        let h = header_line(&state);
        assert!(h.contains("big.log"), "{h}");
        assert!(h.contains("2 lines"), "{h}");
        assert!(h.contains("showing the first 512.0 KB of 3.0 MB"), "{h}");
        assert!(h.contains("invalid encoding"), "{h}");
    }

    #[test]
    fn header_names_the_pixel_size_of_a_picture() {
        let bitmap = abstracttui::widgets::Bitmap::from_fn(4, 2, |_, _| Rgba::rgb(1, 2, 3));
        let state = PreviewState {
            seq: 2,
            path: "/x/p.png".into(),
            name: "p.png".into(),
            size: 2048,
            body: PreviewBody::Image(crate::preview::ImagePreview {
                bitmap: Arc::new(bitmap),
                source_px: (4032, 3024),
                format: "JPEG",
            }),
        };
        let h = header_line(&state);
        assert!(h.contains("JPEG 4032×3024"), "{h}");
    }
}
