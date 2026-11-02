//! Skill attachment picker. The engine List owns scrolling, hit-testing and
//! selection; only Space and the explicit row action change attachments.
use crate::{
    store::{SkillInfo, Store},
    ui::UiCtx,
};
use abstracttui::{
    app::current_viewport,
    prelude::*,
    render::{
        rich::{RichLine, RichText, Span},
        Attrs, Style,
    },
    widgets::{List, Scroll},
};

fn toggle(store: Store, ctx: &UiCtx, ix: usize) {
    let catalog = store.skills_catalog.get_untracked();
    let Some(skill) = catalog.get(ix) else { return };
    if skill.blocked {
        store.notify(format!(
            "skill {} is blocked by the gateway's trust policy",
            skill.name
        ));
        return;
    }
    let mut selected = store.selected_skills.get_untracked();
    if let Some(pos) = selected.iter().position(|name| name == &skill.name) {
        selected.remove(pos);
    } else {
        selected.push(skill.name.clone());
    }
    store.selected_skills.set(selected.clone());
    super::persist_prefs(ctx, |prefs| prefs.skills = selected.clone());
}

fn row(skill: &SkillInfo, attached: bool, t: &TokenSet) -> (String, RichText) {
    let mark = if skill.blocked {
        "[-]"
    } else if attached {
        "[x]"
    } else {
        "[ ]"
    };
    let ink = if skill.blocked {
        t.text_muted
    } else if attached {
        t.accent
    } else {
        t.text
    };
    let attrs = if attached && !skill.blocked {
        Attrs::BOLD
    } else {
        Attrs::default()
    };
    let label = format!("{mark} {}", skill.name);
    let rich = RichText::from_lines(vec![RichLine::from_spans(vec![Span::new(
        label.clone(),
        Style::default().fg(ink).attrs(attrs),
    )])]);
    (label, rich)
}

fn trust_label(skill: &SkillInfo) -> &str {
    match skill.trust.as_str() {
        "adopted" => "Adopted",
        "first-party" | "first_party" => "First-party",
        "" => "Trust not reported",
        other => other,
    }
}

pub fn open(cx: Scope, store: Store, ctx: &UiCtx) {
    let vp = current_viewport();
    let size = Size::new((vp.w - 4).clamp(1, 128), (vp.h - 4).clamp(1, 42));
    let context = ctx.clone();
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let cursor = mcx.signal(0usize);
        let selected_key = mcx.signal(String::new());
        let offset = mcx.signal(0i32);
        let mut root = Element::new()
            .style(
                LayoutStyle::column()
                    .gap(if size.h >= 18 { 1 } else { 0 })
                    .padding(Edges::all(1)),
            )
            .draw(move |canvas, rect| canvas.fill(rect, ' ', t.text, t.surface))
            .shortcut(KeyChord::plain(Key::Escape), {
                let ctx = context.clone();
                move |_| ctx.close_modal()
            })
            .shortcut(KeyChord::plain(Key::Enter), {
                let ctx = context.clone();
                move |_| ctx.close_modal()
            })
            .shortcut(KeyChord::plain(Key::Char(' ')), {
                let ctx = context.clone();
                move |_| toggle(store, &ctx, cursor.get_untracked())
            })
            .child(dyn_view(LayoutStyle::line(1).shrink(0.0), move || {
                let n = store.skills_catalog.with(|catalog| catalog.len());
                let attached = store.selected_skills.with(|skills| skills.len());
                let error = store.skills_error.get();
                let loaded = store.skills_shelf.with(Option::is_some);
                let title = if !error.is_empty() {
                    format!("gateway skills — discovery failed: {error}")
                } else if n == 0 && !loaded {
                    "gateway skills — loading…".into()
                } else if n == 0 {
                    "gateway skills — the shelf is empty".into()
                } else {
                    format!("gateway skills — {n} available · {attached} attached")
                };
                super::modals::title_row(&t, title)
            }))
            .child(dyn_view_scoped(
                LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),
                {
                    let ctx = context.clone();
                    move |scx| {
                        let catalog = store.skills_catalog.get();
                        let attached = store.selected_skills.get();
                        if catalog.is_empty() {
                            let lines = match store.skills_shelf.get() {
                                Some(shelf) => super::modals::empty_shelf_lines(&shelf),
                                None => vec!["Waiting for the Gateway's skill shelf…".into()],
                            };
                            return Scroll::new(MarkdownView::new(lines.join("\n\n")).view(scx))
                                .view(scx);
                        }
                        let (labels, rich): (Vec<_>, Vec<_>) = catalog
                            .iter()
                            .map(|skill| row(skill, attached.contains(&skill.name), &t))
                            .unzip();
                        let keys = catalog
                            .iter()
                            .map(|skill| skill.name.clone())
                            .collect::<Vec<_>>();
                        let actions = catalog
                            .iter()
                            .map(|skill| {
                                if skill.blocked {
                                    "Blocked "
                                } else if attached.contains(&skill.name) {
                                    "Detach "
                                } else {
                                    "Attach "
                                }
                                .to_string()
                            })
                            .collect::<Vec<_>>();
                        let ctx = ctx.clone();
                        let mut list_tokens = t;
                        list_tokens.selection_bg = t.surface_raised;
                        list_tokens.selection_fg = t.text;
                        // No on_activate: Enter closes, Space toggles, browsing only
                        // selects. Row attachment is an explicit pointer action.
                        List::new(labels)
                            .rich_items(rich)
                            .selection(cursor)
                            .selection_key(selected_key)
                            .key_fn(move |ix, _| keys[ix].clone())
                            .offset_y(offset)
                            .row_accessory(move |ix, _| Some(actions[ix].clone()))
                            .accessory_width(9)
                            .on_accessory_click(move |ix| toggle(store, &ctx, ix))
                            .layout(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)))
                            .element(scx, &list_tokens)
                            .autofocus()
                            .build()
                    }
                },
            ));
        if size.h >= 16 {
            root = root.child(dyn_view_scoped(
                LayoutStyle::default()
                    .height(Dimension::Percent(0.4))
                    .shrink(0.0),
                move |scx| {
                    let catalog = store.skills_catalog.get();
                    let attached = store.selected_skills.get();
                    let text = match catalog.get(cursor.get().min(catalog.len().saturating_sub(1)))
                    {
                        Some(skill) => {
                            let state = if skill.blocked {
                                "Blocked by the Gateway's trust policy"
                            } else if attached.contains(&skill.name) {
                                "Attached to your runs"
                            } else {
                                "Not attached"
                            };
                            format!(
                                "**{}** · {} · {state}\n\n{}",
                                skill.name,
                                trust_label(skill),
                                skill.description
                            )
                        }
                        None => String::new(),
                    };
                    Scroll::new(MarkdownView::new(text).view(scx))
                        .scrollbar_auto_hide(true)
                        .view(scx)
                },
            ));
        }
        if size.h >= 12 {
            let mut help_tokens = t;
            help_tokens.text_faint = t.text;
            root = root.child(super::modals::hint_row(
                &help_tokens,
                "↑↓/PgUp/PgDn browse · wheel scroll · space switch · Enter/Esc close".into(),
            ));
        }
        root.build()
    });
}
