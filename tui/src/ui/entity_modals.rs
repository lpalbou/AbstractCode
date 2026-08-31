//! `/entities` — roster + async identity card, and the task-title prompt.
//!
//! The modal opens INSTANTLY on the cached roster ("as of HH:MM —
//! refreshing…") — the live roster fetch can hang for tens of seconds
//! behind the gateway's per-warm-home drives fold, and per-entity reads
//! stay fast meanwhile (measured live 2026-07-22), so the cache is the
//! honest instant surface and the refresh lands asynchronously.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use abstracttui::app::current_viewport;
use abstracttui::prelude::*;
use abstracttui::text;
use abstracttui::widgets::TextInput;

use crate::convo::{self, ConvoStatus};
use crate::entities::EntityInfo;
use crate::runner::Cmd;
use crate::store::Store;
use crate::ui::{entity_actions, UiCtx};

fn modal_size(w: i32, h: i32) -> Size {
    let vp = current_viewport();
    Size::new(w.min(vp.w - 4).max(20), h.min(vp.h - 6).max(6))
}

fn line_row(text_body: String, ink: Rgba) -> View {
    Element::new()
        .style(LayoutStyle::line(1).shrink(0.0))
        .draw(move |canvas, rect| {
            let fitted = text::truncate_ellipsis(&text_body, (rect.w - 1).max(4));
            canvas.print(Point::new(rect.x, rect.y), &fitted, ink, Rgba::TRANSPARENT);
        })
        .build()
}

/// One roster row's text: name · state (+liveness) · pending tasks ·
/// drive ratios when present; broken homes render labeled.
fn roster_row(e: &EntityInfo) -> String {
    if !e.error.is_empty() {
        return format!("{}  — broken home: {}", e.slug, e.error);
    }
    let mut parts = vec![e.slug.clone()];
    let mut state = e.state.clone();
    if !e.liveness.is_empty() && e.liveness != "alive" {
        state.push_str(&format!(" ({})", e.liveness));
    }
    if !state.is_empty() {
        parts.push(state);
    }
    if let Some(n) = e.pending_tasks {
        if n > 0 {
            parts.push(format!("{n} task(s) pending"));
        }
    }
    if let Some(d) = &e.drives {
        let s = d.summary();
        if !s.is_empty() {
            parts.push(s);
        }
    }
    parts.join("  ·  ")
}

/// `/entities [name]` — the roster modal (+ deep link to a card).
pub fn open_entities(cx: Scope, store: Store, ctx: &UiCtx, deep_link: Option<String>) {
    let ctx2 = ctx.clone();
    let size = modal_size(90, 30);
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let start = deep_link
            .as_deref()
            .and_then(|n| {
                let n = n.trim().to_lowercase();
                store
                    .entities
                    .with_untracked(|es| es.iter().position(|e| e.slug == n))
            })
            .unwrap_or(0);
        let cursor = mcx.signal(start);
        // Card fetches already posted from THIS modal (dedup); the store's
        // card cache makes browsing back and forth instant.
        let requested: Rc<RefCell<HashSet<String>>> = Rc::new(RefCell::new(HashSet::new()));

        // Selection → async card load (cache-first, one request per slug).
        {
            let ctx = ctx2.clone();
            let requested = requested.clone();
            mcx.effect(move || {
                let ix = cursor.get();
                let slug = store
                    .entities
                    .with(|es| es.get(ix).filter(|e| e.error.is_empty()).map(|e| e.slug.clone()));
                let Some(slug) = slug else { return };
                let cached = store
                    .entity_cards
                    .with_untracked(|cards| cards.iter().any(|(n, _)| *n == slug));
                if !cached && requested.borrow_mut().insert(slug.clone()) {
                    ctx.send(Cmd::LoadEntityCard { name: slug });
                }
            });
        }

        let selected_slug = move || {
            store.entities.with_untracked(|es| {
                es.get(cursor.get_untracked())
                    .filter(|e| e.error.is_empty())
                    .map(|e| e.slug.clone())
            })
        };

        let move_cursor = move |delta: i64| {
            let n = store.entities.with_untracked(|es| es.len());
            if n == 0 {
                return;
            }
            cursor.update(|c| {
                let cur = *c as i64 + delta;
                *c = cur.clamp(0, n as i64 - 1) as usize;
            });
        };

        // Enter = talk (@name): open/focus the conversation, close modal.
        let talk = {
            let ctx = ctx2.clone();
            move || {
                let Some(slug) = selected_slug() else { return };
                ctx.close_modal();
                entity_actions::open_or_focus(store, &ctx, &slug);
            }
        };
        // t = leave a task: swap to the task-title prompt.
        let task = {
            let ctx = ctx2.clone();
            move || {
                let Some(slug) = selected_slug() else { return };
                open_task_prompt(cx, store, &ctx, slug);
            }
        };
        // e = end the visit (only if one is open with this entity).
        let end = {
            let ctx = ctx2.clone();
            move || {
                let Some(slug) = selected_slug() else { return };
                let has_open = store.convos.with_untracked(|cs| {
                    convo::find(cs, &slug)
                        .map(|ix| {
                            matches!(
                                cs[ix].status,
                                ConvoStatus::Ready | ConvoStatus::Parked | ConvoStatus::TurnRunning
                            )
                        })
                        .unwrap_or(false)
                });
                if has_open {
                    ctx.close_modal();
                    entity_actions::end_visit(store, &ctx, Some(&slug), "");
                } else {
                    store.notify(format!("no open visit with {slug}"));
                }
            }
        };

        Element::new()
            .style(LayoutStyle::column().gap(1).padding(Edges::all(1)))
            .focusable()
            .autofocus()
            .shortcut(KeyChord::plain(Key::Escape), {
                let ctx = ctx2.clone();
                move |_| ctx.close_modal()
            })
            .shortcut(KeyChord::plain(Key::Up), move |_| move_cursor(-1))
            .shortcut(KeyChord::plain(Key::Down), move |_| move_cursor(1))
            .shortcut(KeyChord::plain(Key::Enter), {
                let talk = talk.clone();
                move |_| talk()
            })
            .shortcut(KeyChord::plain(Key::Char('t')), {
                let task = task.clone();
                move |_| task()
            })
            .shortcut(KeyChord::plain(Key::Char('e')), {
                let end = end.clone();
                move |_| end()
            })
            // The footer promises Ctrl+D for provenance — modal layers
            // swallow keys before root shortcuts (engine overlay
            // dispatch), so the binding must live here too.
            .shortcut(KeyChord::new(Mods::CTRL, Key::Char('d')), {
                let ctx = ctx2.clone();
                move |_| crate::ui::toggle_details(store, &ctx)
            })
            // Title: honest freshness (cached "as of" + refreshing/error).
            .child(dyn_view(LayoutStyle::line(1), move || {
                let t2 = abstracttui::app::current_theme().tokens;
                let n = store.entities.with(|es| es.len());
                let as_of = store.entities_as_of.get();
                let loading = store.entities_loading.get();
                let err = store.entities_error.get();
                let mut title = format!("entities — {n} on this gateway");
                if !as_of.is_empty() {
                    title.push_str(&format!(" · as of {as_of} UTC"));
                }
                if loading {
                    // The roster fetch can take tens of seconds (gateway
                    // drives fold) — the cached rows above stay usable.
                    title.push_str(" — refreshing… (roster can be slow; cached rows are live)");
                } else if !err.is_empty() {
                    title.push_str(&format!(" — refresh failed: {err}"));
                }
                line_row(title, t2.accent)
            }))
            // Roster rows (windowed by the granted rect).
            .child(dyn_view(
                LayoutStyle::default()
                    .grow(1.0)
                    .basis(Dimension::Cells(0))
                    .shrink(1.0),
                move || {
                    let t2 = abstracttui::app::current_theme().tokens;
                    let entities = store.entities.get();
                    let cur = cursor.get().min(entities.len().saturating_sub(1));
                    let rows: Vec<(String, bool)> = entities
                        .iter()
                        .enumerate()
                        .map(|(i, e)| (roster_row(e), i == cur))
                        .collect();
                    Element::new()
                        .style(LayoutStyle::column().grow(1.0).basis(Dimension::Cells(0)))
                        .draw(move |canvas, rect| {
                            if rows.is_empty() {
                                canvas.print(
                                    Point::new(rect.x + 1, rect.y),
                                    "no entities cached yet — refreshing from the gateway…",
                                    t2.text_faint,
                                    Rgba::TRANSPARENT,
                                );
                                return;
                            }
                            let h = rect.h.max(1) as usize;
                            let start = if rows.len() <= h {
                                0
                            } else {
                                cur.saturating_sub(h / 2).min(rows.len() - h)
                            };
                            for (line, (text_body, selected)) in
                                rows.iter().skip(start).take(h).enumerate()
                            {
                                let y = rect.y + line as i32;
                                let (ink, bg) = if *selected {
                                    (t2.selection_fg, t2.selection_bg)
                                } else {
                                    (t2.text, Rgba::TRANSPARENT)
                                };
                                if *selected {
                                    canvas.fill(Rect::new(rect.x, y, rect.w, 1), ' ', ink, bg);
                                }
                                let fitted =
                                    text::truncate_ellipsis(text_body, (rect.w - 2).max(4));
                                canvas.print(Point::new(rect.x + 1, y), &fitted, ink, bg);
                            }
                        })
                        .build()
                },
            ))
            // Detail card for the selected entity (async; cache-served).
            .child(dyn_view(
                LayoutStyle::default()
                    .grow(1.4)
                    .basis(Dimension::Cells(0))
                    .shrink(1.0),
                move || {
                    let t2 = abstracttui::app::current_theme().tokens;
                    let details = store.show_details.get();
                    let ix = cursor.get();
                    let entity = store.entities.with(|es| es.get(ix).cloned());
                    let mut lines: Vec<(String, Rgba)> = Vec::new();
                    match entity {
                        None => lines.push(("select an entity above".into(), t2.text_faint)),
                        Some(e) if !e.error.is_empty() => {
                            lines.push((format!("broken home: {}", e.error), t2.error));
                        }
                        Some(e) => {
                            let card = store.entity_cards.with(|cards| {
                                cards.iter().find(|(n, _)| *n == e.slug).map(|(_, c)| c.clone())
                            });
                            match card {
                                None => {
                                    lines.push((
                                        format!("{} — identity card loading…", e.slug),
                                        t2.text_faint,
                                    ));
                                    if !e.handle.is_empty() {
                                        lines.push((format!("handle: {}", e.handle), t2.text_muted));
                                    }
                                }
                                Some(card) => {
                                    let mut head = card.name.clone();
                                    if !card.handle.is_empty() {
                                        head.push_str(&format!("  ·  {}", card.handle));
                                    }
                                    if let Some(days) = card.age_days {
                                        head.push_str(&format!("  ·  {days} days old"));
                                    }
                                    if !card.state.is_empty() {
                                        head.push_str(&format!("  ·  {}", card.state));
                                    }
                                    lines.push((head, t2.text));
                                    for section in &card.sections {
                                        lines.push((format!("— {}", section.title), t2.accent));
                                        for l in section.lines.iter().take(6) {
                                            lines.push((format!("  {l}"), t2.text_muted));
                                        }
                                        if section.lines.len() > 6 {
                                            lines.push((
                                                format!("  (+{} more)", section.lines.len() - 6),
                                                t2.text_faint,
                                            ));
                                        }
                                        // Provenance behind the details toggle.
                                        if details && !section.provenance.is_empty() {
                                            lines.push((
                                                format!("  ⌞ {}", section.provenance),
                                                t2.text_faint,
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Element::new()
                        .style(LayoutStyle::column().grow(1.0).basis(Dimension::Cells(0)))
                        .draw(move |canvas, rect| {
                            for (i, (text_body, ink)) in
                                lines.iter().take(rect.h.max(0) as usize).enumerate()
                            {
                                let fitted =
                                    text::truncate_ellipsis(text_body, (rect.w - 2).max(4));
                                canvas.print(
                                    Point::new(rect.x + 1, rect.y + i as i32),
                                    &fitted,
                                    *ink,
                                    Rgba::TRANSPARENT,
                                );
                            }
                        })
                        .build()
                },
            ))
            .child(line_row(
                "[Enter] talk (@name) · [t] leave a task · [e] end visit · Ctrl+D provenance · Esc closes"
                    .into(),
                t.text_faint,
            ))
            .build()
    });
}

/// The `/task` title prompt (from the roster footer's `t`).
fn open_task_prompt(cx: Scope, store: Store, ctx: &UiCtx, slug: String) {
    let ctx2 = ctx.clone();
    let size = modal_size(70, 9);
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let title = mcx.signal(String::new());
        let send = {
            let ctx = ctx2.clone();
            let slug = slug.clone();
            move |text: String| {
                let text = text.trim().to_string();
                if text.is_empty() {
                    store.notify("a task needs a title — nothing recorded");
                } else {
                    entity_actions::leave_task(store, &ctx, &slug, &text);
                }
                ctx.close_modal();
            }
        };
        Element::new()
            .style(LayoutStyle::column().gap(1).padding(Edges::all(1)))
            .shortcut(KeyChord::plain(Key::Escape), {
                let ctx = ctx2.clone();
                move |_| ctx.close_modal()
            })
            .child(line_row(format!("leave a task on {slug}'s desk"), t.accent))
            .child(line_row(
                "recorded durably; pickup happens at the entity's own boundary".into(),
                t.text_faint,
            ))
            .child(
                TextInput::new()
                    .value(title)
                    .placeholder("task title… (Enter records · Esc cancels)")
                    .on_submit({
                        let send = send.clone();
                        move |text| send(text.to_string())
                    })
                    .layout(LayoutStyle::line(1))
                    .element(mcx, &t)
                    .autofocus()
                    .build(),
            )
            .build()
    });
}
