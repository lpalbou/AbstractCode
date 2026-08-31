//! `/queue` manager modal: browse, remove, reorder, resume, pop-to-composer.
//!
//! Self-contained rendering (its own row painter) so this file never
//! contends with the shared pickers in `modals.rs`. Keys follow the plan
//! (docs/design/plan-interaction-model.md item 1): ↑↓ select, `x` remove,
//! `u`/`d` reorder, `c` clear, `r` resume (paused only), `e` pop the
//! selected prompt into the composer, Esc closes.

use abstracttui::prelude::*;
use abstracttui::text;

use crate::store::Store;
use crate::ui::{queue_preview, UiCtx};

/// Selected item id by cursor position (ids are the stable identity —
/// drains and edits shift positions under a live modal).
fn selected_id(store: Store, cursor: usize) -> Option<u64> {
    store.queue.with_untracked(|q| q.get(cursor).map(|p| p.id))
}

pub fn open_queue(cx: Scope, store: Store, ctx: &UiCtx) {
    let ctx2 = ctx.clone();
    let vp = abstracttui::app::current_viewport();
    let size = Size::new(84.min(vp.w - 4).max(30), 16.min(vp.h - 6).max(8));
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let cursor = mcx.signal(0usize);

        let clamp_cursor = move || {
            let n = store.queue.with_untracked(|q| q.len());
            cursor.update(|c| *c = (*c).min(n.saturating_sub(1)));
        };
        let move_cursor = move |delta: i64| {
            let n = store.queue.with_untracked(|q| q.len());
            if n == 0 {
                return;
            }
            cursor.update(|c| *c = (*c as i64 + delta).clamp(0, n as i64 - 1) as usize);
        };
        let remove_selected = move || {
            let Some(id) = selected_id(store, cursor.get_untracked()) else {
                return;
            };
            store.queue.update(|q| q.retain(|p| p.id != id));
            clamp_cursor();
        };
        // Reorder by one position; the cursor FOLLOWS the item.
        let reorder = move |delta: i64| {
            let ix = cursor.get_untracked();
            let n = store.queue.with_untracked(|q| q.len());
            let to = ix as i64 + delta;
            if n < 2 || to < 0 || to >= n as i64 {
                return;
            }
            store.queue.update(|q| q.swap(ix, to as usize));
            cursor.set(to as usize);
        };

        Element::new()
            .style(LayoutStyle::column().gap(1).padding(Edges::all(1)))
            .focusable()
            .autofocus()
            .shortcut(KeyChord::plain(Key::Escape), {
                let ctx = ctx2.clone();
                move |_| ctx.close_modal()
            })
            .shortcut(KeyChord::plain(Key::Enter), {
                let ctx = ctx2.clone();
                move |_| ctx.close_modal()
            })
            .shortcut(KeyChord::plain(Key::Up), move |_| move_cursor(-1))
            .shortcut(KeyChord::plain(Key::Down), move |_| move_cursor(1))
            .shortcut(KeyChord::plain(Key::Char('x')), move |_| remove_selected())
            .shortcut(KeyChord::plain(Key::Char('u')), move |_| reorder(-1))
            .shortcut(KeyChord::plain(Key::Char('d')), move |_| reorder(1))
            .shortcut(KeyChord::plain(Key::Char('c')), move |_| {
                let n = store.queue.with_untracked(|q| q.len());
                if n > 0 {
                    store.queue.set(Vec::new());
                    store.notify(format!("queue cleared ({n} prompt(s) removed)"));
                }
                cursor.set(0);
            })
            .shortcut(KeyChord::plain(Key::Char('r')), move |_| {
                // Resume is meaningful only when paused; a no-op notify
                // beats a silently-dead key.
                if store.queue_paused.get_untracked() {
                    store.queue_paused.set(false);
                    store.notify("queue resumed");
                } else {
                    store.notify("queue is not paused");
                }
            })
            .shortcut(KeyChord::plain(Key::Char('e')), {
                let ctx = ctx2.clone();
                move |_| {
                    // Pop-to-composer: remove the item and seed the
                    // composer draft (root() drains composer_seed — the
                    // TextAreaState lives in root scope, unreachable from
                    // here). Closing returns focus to the composer.
                    let Some(id) = selected_id(store, cursor.get_untracked()) else {
                        return;
                    };
                    let text = store.queue.with_untracked(|q| {
                        q.iter()
                            .find(|p| p.id == id)
                            .map(|p| p.text.clone())
                            .unwrap_or_default()
                    });
                    store.queue.update(|q| q.retain(|p| p.id != id));
                    store.composer_seed.set(Some(text));
                    ctx.close_modal();
                }
            })
            .child(dyn_view(LayoutStyle::line(1), move || {
                let t2 = abstracttui::app::current_theme().tokens;
                let n = store.queue.with(|q| q.len());
                let paused = store.queue_paused.get();
                let state = if paused {
                    "PAUSED — r resumes"
                } else if n == 0 {
                    "live"
                } else {
                    "live — drains after the current run succeeds"
                };
                let title = format!("prompt queue — {n} waiting · {state}");
                let accent = if paused { t2.warn } else { t2.accent };
                Element::new()
                    .style(LayoutStyle::line(1).shrink(0.0))
                    .draw(move |canvas, rect| {
                        let fitted = text::truncate_ellipsis(&title, (rect.w - 1).max(4));
                        canvas.print(
                            Point::new(rect.x, rect.y),
                            &fitted,
                            accent,
                            Rgba::TRANSPARENT,
                        );
                    })
                    .build()
            }))
            .child(dyn_view(
                LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),
                move || {
                    let t2 = abstracttui::app::current_theme().tokens;
                    let items = store.queue.get();
                    let cur = cursor.get().min(items.len().saturating_sub(1));
                    let rows: Vec<String> = items
                        .iter()
                        .enumerate()
                        .map(|(i, p)| format!("{}. {}", i + 1, queue_preview(&p.text)))
                        .collect();
                    Element::new()
                        .style(LayoutStyle::column().grow(1.0).basis(Dimension::Cells(0)))
                        .draw(move |canvas, rect| {
                            if rows.is_empty() {
                                canvas.print(
                                    Point::new(rect.x + 1, rect.y),
                                    "queue is empty — /queue <text> adds a prompt",
                                    t2.text_faint,
                                    Rgba::TRANSPARENT,
                                );
                                return;
                            }
                            // Window around the cursor (honest edge markers).
                            let h = rect.h.max(1) as usize;
                            let start = if rows.len() <= h {
                                0
                            } else {
                                cur.saturating_sub(h / 2).min(rows.len() - h)
                            };
                            for (line, (ix, row)) in
                                rows.iter().enumerate().skip(start).take(h).enumerate()
                            {
                                let y = rect.y + line as i32;
                                if y >= rect.bottom() {
                                    break;
                                }
                                let is_cursor = ix == cur;
                                let (ink, bg) = if is_cursor {
                                    (t2.text, t2.surface_raised)
                                } else {
                                    (t2.text_muted, Rgba::TRANSPARENT)
                                };
                                if is_cursor {
                                    canvas.fill(
                                        Rect::new(rect.x, y, rect.w, 1),
                                        ' ',
                                        ink,
                                        bg,
                                    );
                                }
                                let fitted =
                                    text::truncate_ellipsis(row, (rect.w - 2).max(4));
                                canvas.print(Point::new(rect.x + 1, y), &fitted, ink, bg);
                            }
                        })
                        .build()
                },
            ))
            .child({
                let faint = t.text_faint;
                Element::new()
                    .style(LayoutStyle::line(1).shrink(0.0))
                    .draw(move |canvas, rect| {
                        let hint = "↑↓ select · x remove · u/d reorder · c clear · r resume · e edit in composer · Esc closes";
                        let fitted = text::truncate_ellipsis(hint, (rect.w - 1).max(4));
                        canvas.print(
                            Point::new(rect.x, rect.y),
                            &fitted,
                            faint,
                            Rgba::TRANSPARENT,
                        );
                    })
                    .build()
            })
            .build()
    });
}
