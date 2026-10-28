//! The status bar's theme control (operator ask 2026-10-10: "the theme
//! [name] clickable, to show a dropdown list with all the available
//! themes that we could rapidly scroll to see how the global UI looks
//! with it … accessible through tab and mouse click/scroll").
//!
//! - The TRIGGER is the theme's name in the status bar, `Tokyo Night ▾`:
//!   since 0.12.0 an action span of the bar (`chrome::ACT_THEME`,
//!   abstracttui 0187) — a click opens, and Tab to the bar, → to the
//!   name, Enter or Space opens. (The wheel over the CLOSED name no
//!   longer steps the theme: the engine does not say which span a wheel
//!   lands on.)
//! - The DROPDOWN lists every theme grouped Dark / Light (the engine's
//!   `themes_by_mode` order), anchored at the trigger (it opens upward
//!   from the bottom bar). Moving the highlight — arrows, Page Up/Down,
//!   Home/End, or the WHEEL — applies that theme LIVE to the whole UI,
//!   the dropdown included. Enter or a click keeps it; Escape restores
//!   the theme that was active at open; a press outside keeps what is
//!   showing. Every kept switch is saved like Ctrl+T's.
//!
//! Built on the engine's public OWNED popup (`app::anchored::Popup`):
//! the engine's own `ThemeSwitcher` has the right menu but a glyph-only
//! trigger and no wheel handling, so the face is ours and the substrate
//! (placement, flip, Escape, outside press, resize) is the engine's.
//!
//! The popup opens on the ROOT scope, not the trigger's: this app
//! rebuilds its chrome on every theme change, and a popup dies with its
//! opener's scope — opened from the trigger, the first preview step
//! would have closed it.

use std::cell::RefCell;
use std::rc::Rc;

use abstracttui::app::anchored::{DismissReason, PanelAnchor, PanelWidth, Popup};
use abstracttui::prelude::*;
use abstracttui::text;
use abstracttui::theme::{themes_by_mode, Theme, ThemeMode};

use super::UiCtx;

/// Rows shown at once before the list windows around the highlight.
pub const MAX_VISIBLE: usize = 14;

/// The dropdown's state, created ONCE in the root (the status bar is
/// rebuilt often; it must survive it): whether the dropdown is open.
#[derive(Copy, Clone)]
pub struct TriggerState {
    pub open: Signal<bool>,
}

impl TriggerState {
    pub fn new(cx: Scope) -> TriggerState {
        TriggerState {
            open: cx.signal(false),
        }
    }
}

/// One dropdown row: a group header (`None`, not selectable) or a theme.
pub type Entry = (Option<&'static Theme>, String);

/// The rows: `☾ Dark` then its themes, `☼ Light` then its themes.
pub fn entries() -> Vec<Entry> {
    let mut out = Vec::new();
    for (mode, glyph) in [(ThemeMode::Dark, "☾"), (ThemeMode::Light, "☼")] {
        out.push((None, format!("{glyph} {}", mode.label())));
        for t in themes_by_mode(mode) {
            out.push((Some(t), t.label.to_string()));
        }
    }
    out
}

/// The next selectable row from `from` in direction `delta` (±1),
/// skipping headers; stays put at either end.
pub fn step(entries: &[Entry], from: usize, delta: i32) -> usize {
    let mut i = from as i64;
    loop {
        i += delta as i64;
        if i < 0 || i >= entries.len() as i64 {
            return from;
        }
        if entries[i as usize].0.is_some() {
            return i as usize;
        }
    }
}

/// `n` selectable steps at once (Page Up / Page Down).
pub fn page(entries: &[Entry], from: usize, delta: i32, n: usize) -> usize {
    let mut at = from;
    for _ in 0..n.max(1) {
        at = step(entries, at, delta);
    }
    at
}

/// The first / last selectable row.
pub fn first(entries: &[Entry]) -> usize {
    entries.iter().position(|e| e.0.is_some()).unwrap_or(0)
}

pub fn last(entries: &[Entry]) -> usize {
    entries.iter().rposition(|e| e.0.is_some()).unwrap_or(0)
}

/// The window's top row keeping `highlight` visible in `visible` rows.
pub fn window_top(top: usize, highlight: usize, visible: usize, len: usize) -> usize {
    let visible = visible.max(1);
    let mut top = top;
    if highlight < top {
        top = highlight;
    } else if highlight >= top + visible {
        top = highlight + 1 - visible;
    }
    // A header just above the highlight comes along when it fits.
    if top > 0 && top == highlight && top + visible <= len {
        top -= 1;
    }
    top.min(len.saturating_sub(visible))
}

/// The theme `delta` steps from `current` in the dropdown's order,
/// wrapping (the closed trigger's wheel).
pub fn neighbour(current: &str, delta: i32) -> &'static Theme {
    let list: Vec<&'static Theme> = entries().into_iter().filter_map(|e| e.0).collect();
    let at = list.iter().position(|t| t.id == current).unwrap_or(0) as i64;
    let n = list.len() as i64;
    list[((at + delta as i64).rem_euclid(n)) as usize]
}

/// The trigger's text: the theme name and the dropdown chevron.
pub fn trigger_text(label: &str) -> String {
    format!(" {label} ▾ ")
}

/// Apply `theme` if it is not the one showing.
fn apply(theme: &'static Theme) {
    if abstracttui::app::current_theme().id != theme.id {
        abstracttui::app::set_theme(theme);
    }
}

/// Apply and SAVE (a kept switch — the same preference Ctrl+T writes).
fn keep(ctx: &UiCtx, theme: &'static Theme) {
    apply(theme);
    crate::ui::save_theme_pref(ctx, theme.id);
}

/// Open the dropdown against `anchor` (the trigger's screen rect).
pub fn open(root: Scope, ctx: &UiCtx, st: TriggerState, anchor: Rect) {
    if st.open.get_untracked() {
        return;
    }
    let viewport = abstracttui::app::current_viewport();
    let pre_open = abstracttui::app::current_theme();
    let entries = Rc::new(entries());
    let seed = entries
        .iter()
        .position(|e| e.0.is_some_and(|t| t.id == pre_open.id))
        .unwrap_or_else(|| first(&entries));
    let room = anchor.y.max(viewport.h - anchor.bottom()).max(1) as usize;
    let visible = entries.len().min(MAX_VISIBLE).min(room);
    let width = entries.iter().map(|e| text::width(&e.1)).max().unwrap_or(8) + 6;
    let slot: Rc<RefCell<Option<Popup>>> = Rc::new(RefCell::new(None));

    let build = {
        let entries = entries.clone();
        let slot = slot.clone();
        move |pcx: Scope, _flipped: bool| -> View {
            let highlight = pcx.signal(seed);
            let top = pcx.signal(window_top(0, seed, visible, entries.len()));
            // Move = highlight + LIVE apply (the preview IS the switch
            // until Escape restores).
            let move_to = {
                let entries = entries.clone();
                Rc::new(move |pos: usize| {
                    highlight.set(pos);
                    top.set(window_top(top.get_untracked(), pos, visible, entries.len()));
                    if let Some(t) = entries[pos].0 {
                        apply(t);
                    }
                })
            };
            let commit = {
                let move_to = move_to.clone();
                let slot = slot.clone();
                Rc::new(move |pos: usize| {
                    move_to(pos);
                    let popup = slot.borrow().clone();
                    if let Some(p) = popup {
                        p.dismiss(DismissReason::Commit);
                    }
                })
            };
            let handler = {
                let entries = entries.clone();
                move |ectx: &mut abstracttui::ui::EventCtx, ev: &abstracttui::ui::UiEvent| {
                    let h = highlight.get_untracked();
                    match ev {
                        abstracttui::ui::UiEvent::Key(k) if k.mods == Mods::NONE => {
                            match k.key {
                                Key::Down => move_to(step(&entries, h, 1)),
                                Key::Up => move_to(step(&entries, h, -1)),
                                Key::PageDown => move_to(page(&entries, h, 1, visible)),
                                Key::PageUp => move_to(page(&entries, h, -1, visible)),
                                Key::Home => move_to(first(&entries)),
                                Key::End => move_to(last(&entries)),
                                Key::Enter | Key::Char(' ') => commit(h),
                                _ => return, // Escape: the substrate's
                            }
                            ectx.stop_propagation();
                        }
                        abstracttui::ui::UiEvent::Mouse(m) => {
                            match m.kind {
                                abstracttui::ui::MouseKind::ScrollUp => {
                                    move_to(step(&entries, h, -1))
                                }
                                abstracttui::ui::MouseKind::ScrollDown => {
                                    move_to(step(&entries, h, 1))
                                }
                                abstracttui::ui::MouseKind::Down(
                                    abstracttui::ui::MouseButton::Left,
                                ) => {
                                    let rect = ectx.current_rect();
                                    let row =
                                        (m.pos.y - rect.y).max(0) as usize + top.get_untracked();
                                    if entries.get(row).is_some_and(|e| e.0.is_some()) {
                                        commit(row);
                                    }
                                }
                                _ => return,
                            }
                            ectx.stop_propagation();
                        }
                        _ => {}
                    }
                }
            };
            let theme = abstracttui::app::use_theme(pcx);
            let rows = entries.clone();
            let pre_id = pre_open.id;
            Element::new()
                .style(
                    LayoutStyle::column()
                        .width(Dimension::Percent(1.0))
                        .height(Dimension::Percent(1.0)),
                )
                .role(abstracttui::ui::Role::Menu)
                .access_label("themes")
                .on(abstracttui::ui::Phase::Bubble, handler)
                .child(dyn_view(
                    LayoutStyle::column()
                        .width(Dimension::Percent(1.0))
                        .height(Dimension::Percent(1.0)),
                    move || {
                        // Tracked: the preview retints the dropdown too —
                        // what you browse in IS the theme.
                        let t = theme.get().tokens;
                        let h = highlight.get();
                        let tp = top.get();
                        let rows = rows.clone();
                        Element::new()
                            .style(
                                LayoutStyle::column()
                                    .width(Dimension::Percent(1.0))
                                    .height(Dimension::Percent(1.0)),
                            )
                            .draw(move |canvas, rect| {
                                canvas.fill(rect, ' ', t.text, t.surface_raised);
                                for (i, (theme, label)) in rows
                                    .iter()
                                    .enumerate()
                                    .skip(tp)
                                    .take(rect.h.max(0) as usize)
                                {
                                    let y = rect.y + (i - tp) as i32;
                                    let on = i == h;
                                    let (fg, bg) = match theme {
                                        None => (t.text_faint, t.surface_raised),
                                        Some(_) if on => (t.selection_fg, t.selection_bg),
                                        Some(_) => (t.text, t.surface_raised),
                                    };
                                    canvas.fill(Rect::new(rect.x, y, rect.w, 1), ' ', fg, bg);
                                    let x = rect.x + if theme.is_some() { 2 } else { 1 };
                                    let fitted =
                                        text::truncate_ellipsis(label, (rect.w - 5).max(4));
                                    canvas.print(Point::new(x, y), &fitted, fg, bg);
                                    // ● = the theme active at open (what
                                    // Escape returns to).
                                    if theme.is_some_and(|th| th.id == pre_id) {
                                        canvas.print(Point::new(rect.right() - 2, y), "●", fg, bg);
                                    }
                                }
                            })
                            .build()
                    },
                ))
                .build()
        }
    };

    let Some(popup) = Popup::open(
        &ctx.overlays,
        root,
        viewport,
        PanelAnchor { rect: anchor },
        PanelWidth::Content {
            min: width.min(viewport.w.max(1)),
            max: width,
        },
        Size::new(width, visible as i32),
        build,
    ) else {
        return;
    };
    st.open.set(true);
    let dctx = ctx.clone();
    popup.on_dismiss(move |reason| {
        // Opened on the ROOT scope, the popup only loses its anchor when
        // the app itself tears down — nothing to restore or save then,
        // and the root's signals may already be gone (writing one panics:
        // found by quitting with the dropdown open).
        if matches!(reason, DismissReason::AnchorGone) || st.open.try_get_untracked().is_none() {
            return;
        }
        st.open.set(false);
        match reason {
            // Abandon the preview: back to what was active at open.
            DismissReason::Escape => apply(pre_open),
            // Every other ending keeps what is showing — save it.
            _ => {
                let now = abstracttui::app::current_theme();
                if now.id != pre_open.id {
                    keep(&dctx, now);
                }
            }
        }
    });
    *slot.borrow_mut() = Some(popup);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_group_every_theme_under_its_mode() {
        let e = entries();
        assert!(e[0].0.is_none() && e[0].1.contains("Dark"));
        let light = e
            .iter()
            .position(|r| r.0.is_none() && r.1.contains("Light"))
            .unwrap();
        assert!(e[1..light]
            .iter()
            .all(|r| r.0.is_some_and(|t| t.mode() == ThemeMode::Dark)));
        assert!(e[light + 1..]
            .iter()
            .all(|r| r.0.is_some_and(|t| t.mode() == ThemeMode::Light)));
        let themes = e.iter().filter(|r| r.0.is_some()).count();
        assert_eq!(
            themes,
            abstracttui::theme::themes().len(),
            "every theme is listed"
        );
    }

    #[test]
    fn movement_skips_headers_and_stops_at_the_ends() {
        let e = entries();
        let light = e
            .iter()
            .position(|r| r.0.is_none() && r.1.contains("Light"))
            .unwrap();
        assert_eq!(step(&e, light - 1, 1), light + 1, "over the Light header");
        assert_eq!(step(&e, light + 1, -1), light - 1);
        assert_eq!(step(&e, first(&e), -1), first(&e), "top stays");
        assert_eq!(step(&e, last(&e), 1), last(&e), "bottom stays");
        assert_eq!(first(&e), 1);
        assert_eq!(page(&e, 1, 1, 3), 4);
    }

    #[test]
    fn the_window_follows_the_highlight() {
        assert_eq!(window_top(0, 5, 10, 30), 0);
        assert_eq!(window_top(0, 12, 10, 30), 3);
        assert_eq!(window_top(10, 4, 10, 30), 3, "the header above comes along");
        assert_eq!(window_top(0, 29, 10, 30), 20);
    }

    #[test]
    fn the_closed_wheel_wraps_through_the_list() {
        let list: Vec<&'static Theme> = entries().into_iter().filter_map(|e| e.0).collect();
        assert_eq!(neighbour(list[0].id, -1).id, list[list.len() - 1].id);
        assert_eq!(neighbour(list[0].id, 1).id, list[1].id);
    }
}
