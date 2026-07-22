//! Modal surfaces: tool approval, ask-user, pickers, tools, help.
//!
//! All modals are focus-trapped `Modal` overlays; one is open at a time
//! (`UiCtx::open_modal` closes the previous). State created inside a modal
//! lives in the modal's scope and dies on close.

use abstracttui::app::current_viewport;
use abstracttui::prelude::*;
use abstracttui::text;
use abstracttui::widgets::{Button, List, Scroll, TextInput};
use serde_json::Value;

use crate::commands::HELP_LINES;
use crate::runner::Cmd;
use crate::store::Store;
use crate::transcript::{PendingWait, WaitKind};
use crate::ui::UiCtx;

fn modal_size(w: i32, h: i32) -> Size {
    let vp = current_viewport();
    // Clamp above the composer + status rows so modal bottoms never
    // interleave with the chrome at small sizes (live finding at 80x24).
    Size::new(w.min(vp.w - 4).max(20), h.min(vp.h - 6).max(6))
}

fn title_row(t: &TokenSet, title: String) -> View {
    let accent = t.accent;
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
}

fn hint_row(t: &TokenSet, hint: String) -> View {
    let faint = t.text_faint;
    Element::new()
        .style(LayoutStyle::line(1).shrink(0.0))
        .draw(move |canvas, rect| {
            // Ellipsize against the real width — long hints hard-clipped
            // at the panel edge at 80 cols (adversary finding 13).
            let fitted = text::truncate_ellipsis(&hint, (rect.w - 1).max(4));
            canvas.print(
                Point::new(rect.x, rect.y),
                &fitted,
                faint,
                Rgba::TRANSPARENT,
            );
        })
        .build()
}

fn wrapped_lines(t: &TokenSet, source: &str, width: i32, ink: Rgba, cap: usize) -> (View, i32) {
    let _ = t;
    let mut lines: Vec<String> = Vec::new();
    for raw in source.lines() {
        lines.extend(text::wrap(raw, width.max(8)));
        if lines.len() > cap {
            break;
        }
    }
    if lines.len() > cap {
        lines.truncate(cap);
        lines.push("… [#TRUNCATION: full arguments in the run ledger]".into());
    }
    let rows = lines.len().max(1) as i32;
    let view = Element::new()
        .style(LayoutStyle::line(rows))
        .draw(move |canvas, rect| {
            for (i, line) in lines.iter().enumerate() {
                canvas.print(
                    Point::new(rect.x, rect.y + i as i32),
                    line,
                    ink,
                    Rgba::TRANSPARENT,
                );
            }
        })
        .build();
    (view, rows)
}

fn pretty_args(args: Option<&Value>) -> String {
    match args {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Tool approval
// ---------------------------------------------------------------------------

pub fn open_approval(cx: Scope, store: Store, ctx: &UiCtx, wait: PendingWait) {
    let tool_calls = match &wait.kind {
        WaitKind::Approval { tool_calls } => tool_calls.clone(),
        _ => return,
    };
    let ctx2 = ctx.clone();
    // Height budget: panel padding 2 + content padding 2 + title 1 + gaps 3
    // + buttons 1 + hint 1 = 10 fixed rows; the args scroll gets the rest.
    let size = modal_size(76, 10 + 2 + (tool_calls.len() as i32 * 6).min(18));
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let run_id = wait.run_id.clone();
        let wait_key = wait.wait_key.clone();
        let restore = wait.clone();

        let approval_title = format!(
            "tool approval — {} call(s) · run {}",
            tool_calls.len(),
            &run_id[..run_id.len().min(8)]
        );
        let decide = {
            let ctx = ctx2.clone();
            let step_id = wait.step_id.clone();
            move |approved: bool| {
                // Optimistic: clear the prompt now; the runner restores it if
                // the resume is refused.
                store.fold.update(|f| {
                    f.wait_answered(&wait_key, &step_id);
                    f.mark_wait_tools(&wait_key, approved);
                });
                ctx.send(Cmd::Resume {
                    run_id: run_id.clone(),
                    wait_key: wait_key.clone(),
                    payload: if approved {
                        serde_json::json!({"approved": true})
                    } else {
                        serde_json::json!({"approved": false, "reason": "Denied by user"})
                    },
                    approved: Some(approved),
                    restore: Box::new(restore.clone()),
                });
                ctx.close_modal();
            }
        };

        let approve = {
            let d = decide.clone();
            move || d(true)
        };
        let deny = {
            let d = decide.clone();
            move || d(false)
        };
        // "Approve all": this batch + every later batch this session
        // (auto-resume without a prompt). Session-scoped, never persisted;
        // /auto turns it off.
        let approve_all = {
            let d = decide.clone();
            move || {
                store.auto_approve.set(true);
                store.notify("auto-approve ON for this session — /auto turns it off");
                d(true);
            }
        };

        let width = size.w - 4;
        let mut body = Element::new().style(LayoutStyle::column().gap(0));
        let mut used = 0;
        for tc in tool_calls.iter() {
            let name = tc.get("name").and_then(Value::as_str).unwrap_or("(tool)");
            body = body.child(title_row(&t, format!("⚙ {name}")));
            used += 1;
            let args = pretty_args(tc.get("arguments"));
            if !args.is_empty() && used < 22 {
                let (view, rows) = wrapped_lines(&t, &args, width, t.text_muted, 8);
                body = body.child(view);
                used += rows;
            }
        }

        let content_h = (used + 1).max(2);
        Element::new()
            .style(LayoutStyle::column().gap(1).padding(Edges::all(1)))
            .focusable()
            .autofocus()
            .shortcut(KeyChord::plain(Key::Char('a')), {
                let a = approve.clone();
                move |_| a()
            })
            .shortcut(KeyChord::plain(Key::Char('A')), {
                let aa = approve_all.clone();
                move |_| aa()
            })
            .shortcut(KeyChord::plain(Key::Char('d')), {
                let d = deny.clone();
                move |_| d()
            })
            .shortcut(KeyChord::plain(Key::Escape), {
                // Esc DEFERS (the run keeps waiting durably); `d` is the only
                // deny — a dismissal must never tell the model "denied".
                let ctx = ctx2.clone();
                let step_id = wait.step_id.clone();
                move |_| {
                    *ctx.dismissed_wait.borrow_mut() = Some(step_id.clone());
                    ctx.close_modal();
                }
            })
            .child(title_row(&t, approval_title))
            .child(
                Scroll::new(body.build())
                    .content_size(width, content_h)
                    // basis 0: the scroll absorbs ALL flex pressure so the
                    // fixed rows (title/buttons/hint) never shrink away.
                    .layout(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)))
                    .element(mcx, &t)
                    // Focus the args scroll so ↑↓ page long arguments; the
                    // a/d/Esc shortcuts live on the root (still on the path).
                    .autofocus()
                    .build(),
            )
            .child(
                Element::new()
                    .style(LayoutStyle::row().h(1).gap(2).shrink(0.0))
                    .child(Button::new("approve (a)").on_click(approve).view(mcx))
                    .child(
                        Button::new("approve all (A)")
                            .on_click(approve_all)
                            .view(mcx),
                    )
                    .child(Button::new("deny (d)").on_click(deny).view(mcx))
                    .build(),
            )
            .child(hint_row(
                &t,
                "a approve · A approve all (session) · d deny · ↑↓ scroll · Esc defers".into(),
            ))
            .build()
    });
}

// ---------------------------------------------------------------------------
// Ask-user
// ---------------------------------------------------------------------------

pub fn open_ask(cx: Scope, store: Store, ctx: &UiCtx, wait: PendingWait) {
    let prompt = match &wait.kind {
        WaitKind::Ask { prompt } => prompt.clone(),
        _ => return,
    };
    let ctx2 = ctx.clone();
    let size = modal_size(70, 13);
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let run_id = wait.run_id.clone();
        let wait_key = wait.wait_key.clone();
        let answer = mcx.signal(String::new());

        let send = {
            let ctx = ctx2.clone();
            let run_id = run_id.clone();
            let wait_key = wait_key.clone();
            let restore = wait.clone();
            let step_id = wait.step_id.clone();
            move |text: String| {
                store.fold.update(|f| f.wait_answered(&wait_key, &step_id));
                ctx.send(Cmd::Resume {
                    run_id: run_id.clone(),
                    wait_key: wait_key.clone(),
                    payload: serde_json::json!({"response": text}),
                    approved: None,
                    restore: Box::new(restore.clone()),
                });
                ctx.close_modal();
            }
        };

        let (prompt_view, _rows) = wrapped_lines(&t, &prompt, size.w - 4, t.text, 5);
        Element::new()
            .style(LayoutStyle::column().gap(1).padding(Edges::all(1)))
            .child(title_row(&t, "the agent asks".into()))
            .child(prompt_view)
            .child(
                TextInput::new()
                    .value(answer)
                    .placeholder("your answer… (Enter sends)")
                    .on_submit({
                        let send = send.clone();
                        move |text| send(text.to_string())
                    })
                    .layout(LayoutStyle::line(1))
                    .element(mcx, &t)
                    .autofocus()
                    .build(),
            )
            .child(hint_row(
                &t,
                "Enter answers · Esc keeps the run waiting".into(),
            ))
            .shortcut(KeyChord::plain(Key::Escape), {
                let ctx = ctx2.clone();
                move |_| {
                    // Leaving the wait pending is legitimate: the run stays
                    // durable server-side. Re-open via the activity strip.
                    ctx.close_modal();
                }
            })
            .build()
    });
}

// ---------------------------------------------------------------------------
// Pickers
// ---------------------------------------------------------------------------

pub fn open_theme_picker(cx: Scope, store: Store, ctx: &UiCtx) {
    let themes = abstracttui::theme::themes();
    let labels: Vec<String> = themes
        .iter()
        .map(|th| format!("{}{}", th.label, if th.dark { "" } else { "  (light)" }))
        .collect();
    let original = abstracttui::app::current_theme().id;
    let start = themes.iter().position(|th| th.id == original).unwrap_or(0);
    let ctx2 = ctx.clone();
    let size = modal_size(44, (labels.len() as i32 + 7).min(26));
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let selection = mcx.signal(start);
        // Live preview: moving the selection applies the theme immediately.
        mcx.effect(move || {
            let ix = selection.get();
            if let Some(th) = abstracttui::theme::themes().get(ix) {
                abstracttui::app::set_theme_by_id(th.id);
            }
        });
        let confirm = {
            let ctx = ctx2.clone();
            move || {
                let ix = selection.get_untracked();
                if let Some(th) = abstracttui::theme::themes().get(ix) {
                    abstracttui::app::set_theme_by_id(th.id);
                    crate::ui::save_theme_pref(&ctx, th.id);
                    store.notify(format!("theme: {}", th.label));
                }
                ctx.close_modal();
            }
        };
        let cancel = {
            let ctx = ctx2.clone();
            move || {
                abstracttui::app::set_theme_by_id(original);
                ctx.close_modal();
            }
        };
        Element::new()
            .style(LayoutStyle::column().gap(1).padding(Edges::all(1)))
            .shortcut(KeyChord::plain(Key::Escape), {
                let c = cancel.clone();
                move |_| c()
            })
            .child(title_row(
                &t,
                "theme — ↑↓ previews live · Enter keeps · Esc reverts".into(),
            ))
            .child(
                // `on_activate` (0.2.1): Enter/Space/click-on-selected
                // confirm; `on_select` stays movement-only, so the live
                // preview keeps riding the selection signal effect.
                List::new(labels.clone())
                    .selection(selection)
                    .on_activate(move |_ix| confirm())
                    .layout(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)))
                    .element(mcx, &t)
                    .autofocus()
                    .build(),
            )
            .build()
    });
}

pub fn open_workflow_picker(cx: Scope, store: Store, ctx: &UiCtx) {
    let workflows = store.workflows.get_untracked();
    if workflows.is_empty() {
        store.notify("no agent workflows discovered yet (is the gateway up?)");
        return;
    }
    let current = store.workflow.get_untracked();
    let labels: Vec<String> = workflows
        .iter()
        .map(|w| {
            let marker = if w.bundle_id == current.bundle_id && w.flow_id == current.flow_id {
                "● "
            } else {
                "  "
            };
            let desc = if w.description.is_empty() {
                String::new()
            } else {
                format!(" — {}", text::truncate_ellipsis(&w.description, 46))
            };
            format!(
                "{marker}{} ({}:{}){desc}",
                w.label(),
                w.bundle_id,
                w.flow_id
            )
        })
        .collect();
    let start = workflows
        .iter()
        .position(|w| w.bundle_id == current.bundle_id && w.flow_id == current.flow_id)
        .unwrap_or(0);
    let ctx2 = ctx.clone();
    let size = modal_size(84, (labels.len() as i32 + 7).min(24));
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let selection = mcx.signal(start);
        let choose = {
            let ctx = ctx2.clone();
            let workflows = workflows.clone();
            move || {
                let ix = selection.get_untracked();
                if let Some(w) = workflows.get(ix) {
                    store.workflow.set(w.clone());
                    crate::ui::persist_prefs(&ctx, |p| {
                        p.bundle_id = Some(w.bundle_id.clone());
                        p.flow_id = Some(w.flow_id.clone());
                    });
                    store.notify(format!("workflow: {}", w.label()));
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
            .child(title_row(
                &t,
                "agent workflow — ↑↓ browse · Enter selects · Esc closes".into(),
            ))
            .child(
                // `on_activate` (0.2.1): Enter/Space/click-on-selected
                // confirm — the old root-Enter shortcut is gone, and the
                // engine completes its bookkeeping before the callback,
                // so choosing (which closes the modal) is disposal-safe.
                List::new(labels.clone())
                    .selection(selection)
                    .on_activate(move |_ix| choose())
                    .layout(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)))
                    .element(mcx, &t)
                    .autofocus()
                    .build(),
            )
            .build()
    });
}

/// Stage 1: pick a provider (or reset to gateway defaults). Stage 2 (for a
/// provider with models) picks the model. Empty provider/model strings mean
/// "the gateway routes" — the default posture. Arrows browse; Enter chooses.
pub fn open_model_picker(cx: Scope, store: Store, ctx: &UiCtx) {
    let providers = store.providers.get_untracked();
    if providers.is_empty() {
        store.notify("no providers discovered yet — /model again after the gateway catalog loads");
    }
    let cur_provider = store.provider.get_untracked();
    let mut labels: Vec<String> = vec![format!(
        "{}gateway defaults (no override — the gateway routes)",
        if cur_provider.is_empty() {
            "● "
        } else {
            "  "
        }
    )];
    for p in &providers {
        let marker = if p.name == cur_provider { "● " } else { "  " };
        let count = if p.models.is_empty() {
            String::new()
        } else {
            format!("  ({} models)", p.models.len())
        };
        labels.push(format!("{marker}{}{count}", p.name));
    }
    let start = if cur_provider.is_empty() {
        0
    } else {
        providers
            .iter()
            .position(|p| p.name == cur_provider)
            .map(|i| i + 1)
            .unwrap_or(0)
    };
    let ctx2 = ctx.clone();
    let size = modal_size(64, (labels.len() as i32 + 7).min(26));
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let selection = mcx.signal(start);
        let choose = {
            let ctx = ctx2.clone();
            let providers = providers.clone();
            move || {
                let ix = selection.get_untracked();
                if ix == 0 {
                    apply_route(store, &ctx, "", "");
                    ctx.close_modal();
                    return;
                }
                let Some(p) = providers.get(ix - 1).cloned() else {
                    ctx.close_modal();
                    return;
                };
                if p.models.is_empty() {
                    apply_route(store, &ctx, &p.name, "");
                    ctx.close_modal();
                    return;
                }
                // Stage 2 — this provider's models. Synchronous on purpose:
                // open_modal replaces stage 1 atomically (its layer leaves
                // input routing NOW; only scope disposal is deferred), so
                // there is no tick where Enter was consumed but the model
                // list is not yet the one receiving keys. The deferred
                // variant left a stale stage-1 layer eating the first
                // arrows/Enter aimed at stage 2 (live 2026-07-21).
                open_model_stage(cx, store, &ctx, p);
            }
        };
        Element::new()
            .style(LayoutStyle::column().gap(1).padding(Edges::all(1)))
            .shortcut(KeyChord::plain(Key::Escape), {
                let ctx = ctx2.clone();
                move |_| ctx.close_modal()
            })
            .child(title_row(
                &t,
                "provider — ↑↓ browse · Enter opens/selects · Esc closes".into(),
            ))
            .child(
                // `on_activate` (0.2.1): Enter/Space/click-on-selected.
                // Opening stage 2 from inside the callback is safe — the
                // List completes its bookkeeping before invoking it.
                List::new(labels.clone())
                    .selection(selection)
                    .on_activate(move |_ix| choose())
                    .layout(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)))
                    .element(mcx, &t)
                    .autofocus()
                    .build(),
            )
            .build()
    });
}

fn apply_route(store: Store, ctx: &UiCtx, provider: &str, model: &str) {
    store.provider.set(provider.to_string());
    store.model.set(model.to_string());
    let (p, m) = (provider.to_string(), model.to_string());
    crate::ui::persist_prefs(ctx, |prefs| {
        prefs.provider = Some(p.clone());
        prefs.model = Some(m.clone());
    });
    let label = match (provider.is_empty(), model.is_empty()) {
        (true, _) => "gateway defaults".to_string(),
        (false, true) => format!("{provider} (provider default model)"),
        (false, false) => format!("{provider} · {model}"),
    };
    store.notify(format!("route: {label}"));
}

fn open_model_stage(cx: Scope, store: Store, ctx: &UiCtx, provider: crate::store::ProviderInfo) {
    let cur_model = store.model.get_untracked();
    let same_provider = store.provider.get_untracked() == provider.name;
    let mut labels: Vec<String> = vec![format!(
        "{}provider default (let {} decide)",
        if same_provider && cur_model.is_empty() {
            "● "
        } else {
            "  "
        },
        provider.name
    )];
    for m in &provider.models {
        let marker = if same_provider && *m == cur_model {
            "● "
        } else {
            "  "
        };
        labels.push(format!("{marker}{m}"));
    }
    let start = if same_provider && !cur_model.is_empty() {
        provider
            .models
            .iter()
            .position(|m| *m == cur_model)
            .map(|i| i + 1)
            .unwrap_or(0)
    } else {
        0
    };
    let ctx2 = ctx.clone();
    let size = modal_size(70, (labels.len() as i32 + 7).min(28));
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let selection = mcx.signal(start);
        let choose = {
            let ctx = ctx2.clone();
            let provider = provider.clone();
            move || {
                let ix = selection.get_untracked();
                if ix == 0 {
                    apply_route(store, &ctx, &provider.name, "");
                } else if let Some(m) = provider.models.get(ix - 1) {
                    apply_route(store, &ctx, &provider.name, m);
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
            .child(title_row(
                &t,
                format!(
                    "{} models — ↑↓ browse · Enter selects · Esc closes",
                    provider.name
                ),
            ))
            .child(
                // `on_activate` (0.2.1): Enter/Space/click-on-selected.
                List::new(labels.clone())
                    .selection(selection)
                    .on_activate(move |_ix| choose())
                    .layout(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)))
                    .element(mcx, &t)
                    .autofocus()
                    .build(),
            )
            .build()
    });
}

// ---------------------------------------------------------------------------
// Windowed selectable rows (tools / skills / sessions / mcp)
// ---------------------------------------------------------------------------
//
// A hand-rolled row surface instead of `List`/`Scroll`: multi-select needs
// live checkbox re-render on toggle, and `List::on_select` fires on plain
// arrow movement BY DESIGN (0.2.0 kept it as the selection-changed
// notification; the new `on_activate` fires on Enter AND Space, so a
// List-based multi-select could not tell Space-toggles from Enter-closes
// either) — a toggle-on-move list would flip checkboxes while browsing.
// Here the modal ROOT owns focus + keys; rows are pure draws windowed by
// a cursor signal.

/// One rendered row: `header` rows are group labels (not selectable).
#[derive(Clone)]
struct RowSpec {
    text: String,
    header: bool,
    checked: Option<bool>,
    dim: bool,
}

fn draw_rows(rows: Vec<RowSpec>, cursor: usize, selectable: Vec<usize>) -> View {
    let t = abstracttui::app::current_theme().tokens;
    let cursor_row = selectable.get(cursor).copied();
    Element::new()
        .style(LayoutStyle::column().grow(1.0).basis(Dimension::Cells(0)))
        .draw(move |canvas, rect| {
            // Window against the RECT the layout actually granted — never a
            // precomputed height (live defect: chrome-row arithmetic drifted
            // from the real flex result, so the bottom rows were silently
            // cut and the window never scrolled because it believed
            // everything fit).
            let h = rect.h.max(1) as usize;
            let anchor = cursor_row.unwrap_or(0);
            let start = if rows.len() <= h {
                0
            } else {
                anchor.saturating_sub(h / 2).min(rows.len() - h)
            };
            let shown = h.min(rows.len() - start);
            let cut_above = start;
            let cut_below = rows.len() - start - shown;
            for (line, (row_ix, row)) in rows.iter().enumerate().skip(start).take(h).enumerate() {
                let y = rect.y + line as i32;
                if y >= rect.bottom() {
                    break;
                }
                // Honest overflow markers on the window's edge rows: more
                // rows exist above/below (a silently cut list read as
                // "the rest is missing" — live finding).
                let edge_note = if line == 0 && cut_above > 0 {
                    Some(format!("↑ {cut_above} more"))
                } else if line + 1 == shown && cut_below > 0 {
                    Some(format!("↓ {cut_below} more"))
                } else {
                    None
                };
                if let Some(msg) = edge_note {
                    canvas.print(
                        Point::new(rect.x + 2, y),
                        &msg,
                        t.text_faint,
                        Rgba::TRANSPARENT,
                    );
                    continue;
                }
                let is_cursor = cursor_row == Some(row_ix) && !row.header;
                let bg = if is_cursor {
                    t.selection_bg
                } else {
                    Rgba::TRANSPARENT
                };
                if is_cursor {
                    canvas.fill(Rect::new(rect.x, y, rect.w, 1), ' ', t.selection_fg, bg);
                }
                let ink = if is_cursor {
                    t.selection_fg
                } else if row.header {
                    t.accent
                } else if row.dim {
                    t.text_faint
                } else if row.checked == Some(false) {
                    t.text_muted
                } else {
                    t.text
                };
                let marker = match row.checked {
                    Some(true) => "[✓] ",
                    Some(false) => "[ ] ",
                    None => "",
                };
                let prefix = if row.header { "" } else { "  " };
                let fitted = text::truncate_ellipsis(
                    &format!("{prefix}{marker}{}", row.text),
                    (rect.w - 1).max(4),
                );
                canvas.print(Point::new(rect.x, y), &fitted, ink, bg);
            }
        })
        .build()
}

/// `/tools` — enable/disable gateway tools for this client's runs.
/// Untouched = the workflow's own defaults; once customized, the CHECKED
/// set is exactly the allowlist sent with every run (`input_data.tools`).
pub fn open_tools(cx: Scope, store: Store, ctx: &UiCtx) {
    let ctx2 = ctx.clone();
    let size = modal_size(84, 26);
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let cursor = mcx.signal(0usize);

        let toggle = {
            let ctx = ctx2.clone();
            move |all: bool, on: Option<bool>| {
                let tools = store.tools.get_untracked();
                if tools.is_empty() {
                    return;
                }
                // Any edit PRUNES stale names (tools the gateway no longer
                // serves): a leftover disabled name from another gateway
                // must not silently hold the client in allowlist mode
                // (adversary finding 6) or skew counts (finding 2).
                let mut disabled: Vec<String> = store
                    .disabled_tools
                    .get_untracked()
                    .into_iter()
                    .filter(|d| tools.iter().any(|tl| tl.name == *d))
                    .collect();
                if all {
                    disabled = if on == Some(true) {
                        Vec::new()
                    } else {
                        tools.iter().map(|x| x.name.clone()).collect()
                    };
                } else {
                    let ix = cursor.get_untracked();
                    let Some(tool) = tools.get(ix) else { return };
                    if let Some(pos) = disabled.iter().position(|d| *d == tool.name) {
                        disabled.remove(pos);
                    } else {
                        disabled.push(tool.name.clone());
                    }
                }
                store.disabled_tools.set(disabled.clone());
                crate::ui::persist_prefs(&ctx, |p| p.disabled_tools = disabled.clone());
            }
        };

        let move_cursor = move |delta: i64| {
            let n = store.tools.with_untracked(|tl| tl.len());
            if n == 0 {
                return;
            }
            cursor.update(|c| {
                let cur = *c as i64 + delta;
                *c = cur.clamp(0, n as i64 - 1) as usize;
            });
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
            .shortcut(KeyChord::plain(Key::Char(' ')), {
                let tg = toggle.clone();
                move |_| tg(false, None)
            })
            .shortcut(KeyChord::plain(Key::Char('a')), {
                let tg = toggle.clone();
                move |_| tg(true, Some(true))
            })
            .shortcut(KeyChord::plain(Key::Char('n')), {
                let tg = toggle.clone();
                move |_| tg(true, Some(false))
            })
            .child(dyn_view(LayoutStyle::line(1), move || {
                let t2 = abstracttui::app::current_theme().tokens;
                let err = store.tools_error.get();
                // Count only disabled names that EXIST in the inventory:
                // stale names from another gateway must never skew the
                // arithmetic (a raw `n - off` underflowed to u64::MAX in
                // release — adversary finding 2).
                let (n, off) = store.tools.with(|tl| {
                    let off = store
                        .disabled_tools
                        .with(|d| d.iter().filter(|x| tl.iter().any(|t| t.name == **x)).count());
                    (tl.len(), off)
                });
                let title = if !err.is_empty() {
                    format!("gateway tools — discovery failed: {err}")
                } else if n == 0 {
                    "gateway tools — loading…".to_string()
                } else if off == 0 {
                    // "All checked" ≠ the workflow's own pin: the flow's
                    // baked tool set decides when the client sends nothing.
                    format!("gateway tools — {n} available (untouched: the workflow's own tool set decides)")
                } else {
                    format!(
                        "gateway tools — {} on / {off} off (explicit allowlist replaces workflow defaults)",
                        n.saturating_sub(off)
                    )
                };
                title_row(&t2, title)
            }))
            .child(dyn_view(
                LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),
                move || {
                    let tools = store.tools.get();
                    let disabled = store.disabled_tools.get();
                    let cur = cursor.get();
                    let mut rows = Vec::new();
                    let mut selectable = Vec::new();
                    let mut last_group = String::from("\u{0}");
                    for tool in &tools {
                        if tool.toolset != last_group {
                            last_group = tool.toolset.clone();
                            let label = if last_group.is_empty() {
                                "other".to_string()
                            } else {
                                last_group.clone()
                            };
                            rows.push(RowSpec {
                                text: label,
                                header: true,
                                checked: None,
                                dim: false,
                            });
                        }
                        let on = !disabled.contains(&tool.name);
                        selectable.push(rows.len());
                        rows.push(RowSpec {
                            text: format!("{}  {}", tool.name, tool.description),
                            header: false,
                            checked: Some(on),
                            dim: !on,
                        });
                    }
                    if rows.is_empty() {
                        rows.push(RowSpec {
                            text: "no tools discovered yet".into(),
                            header: false,
                            checked: None,
                            dim: true,
                        });
                    }
                    // Clamp the anchor if the inventory shrank mid-modal.
                    let cur = cur.min(selectable.len().saturating_sub(1));
                    draw_rows(rows, cur, selectable)
                },
            ))
            .child(hint_row(
                &t,
                "↑↓ move · Space toggles · a all on · n all off · Enter/Esc closes".into(),
            ))
            .child(hint_row(
                &t,
                "untouched = workflow defaults; customized = checked set is the run's exact tools"
                    .into(),
            ))
            .build()
    });
}

/// `/skills` — attach gateway skills to every run (`input_data.skills`).
pub fn open_skills(cx: Scope, store: Store, ctx: &UiCtx) {
    let ctx2 = ctx.clone();
    let size = modal_size(84, 24);
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let cursor = mcx.signal(0usize);

        let toggle = {
            let ctx = ctx2.clone();
            move || {
                let catalog = store.skills_catalog.get_untracked();
                let ix = cursor.get_untracked();
                let Some(skill) = catalog.get(ix) else { return };
                if skill.blocked {
                    store.notify(format!(
                        "skill {} is blocked by the gateway's trust policy",
                        skill.name
                    ));
                    return;
                }
                let mut selected = store.selected_skills.get_untracked();
                if let Some(pos) = selected.iter().position(|s| *s == skill.name) {
                    selected.remove(pos);
                } else {
                    selected.push(skill.name.clone());
                }
                store.selected_skills.set(selected.clone());
                crate::ui::persist_prefs(&ctx, |p| p.skills = selected.clone());
            }
        };
        let move_cursor = move |delta: i64| {
            let n = store.skills_catalog.with_untracked(|c| c.len());
            if n == 0 {
                return;
            }
            cursor.update(|c| {
                let cur = *c as i64 + delta;
                *c = cur.clamp(0, n as i64 - 1) as usize;
            });
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
            .shortcut(KeyChord::plain(Key::Char(' ')), move |_| toggle())
            .child(dyn_view(LayoutStyle::line(1), move || {
                let t2 = abstracttui::app::current_theme().tokens;
                let n = store.skills_catalog.with(|c| c.len());
                let err = store.skills_error.get();
                let on = store.selected_skills.with(|s| s.len());
                let title = if !err.is_empty() {
                    format!("gateway skills — discovery failed: {err}")
                } else if n == 0 {
                    "gateway skills — loading…".to_string()
                } else {
                    format!("gateway skills — {n} on the shelf · {on} attached to your runs")
                };
                title_row(&t2, title)
            }))
            .child(dyn_view(
                LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),
                move || {
                    let catalog = store.skills_catalog.get();
                    let selected = store.selected_skills.get();
                    let cur = cursor.get();
                    let mut rows = Vec::new();
                    let mut selectable = Vec::new();
                    for skill in &catalog {
                        let on = selected.contains(&skill.name);
                        let trust = if skill.blocked {
                            "BLOCKED".to_string()
                        } else {
                            skill.trust.clone()
                        };
                        selectable.push(rows.len());
                        rows.push(RowSpec {
                            text: format!("{} ({trust})  {}", skill.name, skill.description),
                            header: false,
                            checked: Some(on),
                            dim: skill.blocked || !on,
                        });
                    }
                    if rows.is_empty() {
                        rows.push(RowSpec {
                            text: "no skills on this gateway".into(),
                            header: false,
                            checked: None,
                            dim: true,
                        });
                    }
                    // Clamp the anchor if the shelf shrank mid-modal.
                    let cur = cur.min(selectable.len().saturating_sub(1));
                    draw_rows(rows, cur, selectable)
                },
            ))
            .child(hint_row(
                &t,
                "↑↓ move · Space attach/detach · Enter/Esc closes".into(),
            ))
            .child(hint_row(
                &t,
                "attached skills ride each run as input_data.skills (resolved gateway-side)".into(),
            ))
            .build()
    });
}

/// `/sessions` — pick a recent session to continue (durable server-side).
pub fn open_sessions(cx: Scope, store: Store, ctx: &UiCtx) {
    let entries = ctx.prefs.borrow().recent_sessions.clone();
    if entries.is_empty() {
        store.notify("no remembered sessions yet — /new mints one");
        return;
    }
    let current = store.session_id.get_untracked();
    let labels: Vec<String> = entries
        .iter()
        .map(|e| {
            let marker = if e.id == current { "● " } else { "  " };
            let when = e
                .last_used
                .get(5..16)
                .map(|s| s.replace('T', " "))
                .unwrap_or_default();
            let label = if e.label.is_empty() {
                "(no prompt yet)".to_string()
            } else {
                e.label.clone()
            };
            format!("{marker}{}  {when}  {label}", e.id)
        })
        .collect();
    let start = entries.iter().position(|e| e.id == current).unwrap_or(0);
    let ctx2 = ctx.clone();
    // Height: padding 2 + title 1 + hint 1 + inter-child gaps 2 = 6 fixed
    // rows; every session needs its own line on top of that.
    let size = modal_size(84, (labels.len() as i32 + 8).min(22));
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let selection = mcx.signal(start);
        let choose = {
            let ctx = ctx2.clone();
            let entries = entries.clone();
            move || {
                let ix = selection.get_untracked();
                if let Some(e) = entries.get(ix) {
                    crate::ui::switch_session(store, &ctx, &e.id);
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
            .child(title_row(
                &t,
                "sessions — ↑↓ browse · Enter continues · Esc closes".into(),
            ))
            .child(
                // `on_activate` (0.2.1): Enter/Space/click-on-selected.
                List::new(labels.clone())
                    .selection(selection)
                    .on_activate(move |_ix| choose())
                    .layout(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)))
                    .element(mcx, &t)
                    .autofocus()
                    .build(),
            )
            .child(hint_row(
                &t,
                "memory is durable on the gateway; switching reattaches to a live run if one exists".into(),
            ))
            .build()
    });
}

/// `/mcp` — the gateway's MCP server registry (read-only; gateway-owned).
pub fn open_mcp(cx: Scope, store: Store, ctx: &UiCtx) {
    let ctx2 = ctx.clone();
    let size = modal_size(84, 18);
    ctx.open_modal(cx, size, move |_mcx| {
        let t = abstracttui::app::current_theme().tokens;
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
            .child(dyn_view(LayoutStyle::line(1), move || {
                let t2 = abstracttui::app::current_theme().tokens;
                let n = store.mcp_servers.with(|s| s.len());
                title_row(
                    &t2,
                    format!("MCP servers on the gateway — {n} declared · Esc closes"),
                )
            }))
            .child(dyn_view(
                LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),
                move || {
                    let servers = store.mcp_servers.get();
                    let note = store.mcp_note.get();
                    let mut rows = Vec::new();
                    for s in &servers {
                        let auth = if s.auth_required {
                            " (auth required)"
                        } else {
                            ""
                        };
                        rows.push(RowSpec {
                            text: format!("{}  {}{auth}", s.name, s.url),
                            header: true,
                            checked: None,
                            dim: false,
                        });
                        if !s.description.is_empty() {
                            rows.push(RowSpec {
                                text: format!("  {}", s.description),
                                header: false,
                                checked: None,
                                dim: true,
                            });
                        }
                    }
                    if servers.is_empty() {
                        rows.push(RowSpec {
                            text: "none declared on this gateway".into(),
                            header: false,
                            checked: None,
                            dim: true,
                        });
                        if !note.is_empty() {
                            for line in text::wrap(&note, 78) {
                                rows.push(RowSpec {
                                    text: line,
                                    header: false,
                                    checked: None,
                                    dim: true,
                                });
                            }
                        }
                    }
                    draw_rows(rows, usize::MAX, Vec::new())
                },
            ))
            .child(hint_row(
                &t,
                "MCP servers are gateway configuration; their tools appear in /tools once declared"
                    .into(),
            ))
            .build()
    });
}

/// `/cache` — prompt-cache + context posture for the effective route.
pub fn open_cache(cx: Scope, store: Store, ctx: &UiCtx) {
    let ctx2 = ctx.clone();
    let size = modal_size(78, 15);
    ctx.open_modal(cx, size, move |_mcx| {
        let t = abstracttui::app::current_theme().tokens;
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
            .child(title_row(&t, "prompt cache + context · Esc closes".into()))
            .child(dyn_view(
                LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),
                move || {
                    let cache = store.cache.get();
                    let (dp, dm) = store.default_route.get();
                    let provider = store.provider.get();
                    let model = store.model.get();
                    let stats = store.fold.with(|f| f.stats.clone());
                    let mut rows: Vec<RowSpec> = Vec::new();
                    let mut line = |text: String, dim: bool| {
                        rows.push(RowSpec {
                            text,
                            header: false,
                            checked: None,
                            dim,
                        })
                    };
                    let route = if !provider.is_empty() || !model.is_empty() {
                        format!("{provider} · {model} (your override)")
                    } else if !dp.is_empty() {
                        format!("{dp} · {dm} (gateway default route)")
                    } else {
                        "unresolved (gateway defaults; route not reported yet)".to_string()
                    };
                    line(format!("route      {route}"), false);
                    // Always name the pair the probe ASKED ABOUT — a verdict
                    // without its subject can silently describe a different
                    // route than the line above (adversary finding 5).
                    match &cache {
                        Some(c) if c.supported => line(
                            format!(
                                "cache      supported ({} mode) on {} · {} — runs enable it automatically",
                                if c.mode.is_empty() {
                                    "provider"
                                } else {
                                    &c.mode
                                },
                                c.provider,
                                if c.model.is_empty() {
                                    "(provider default)"
                                } else {
                                    &c.model
                                }
                            ),
                            false,
                        ),
                        Some(c) => line(
                            format!(
                                "cache      not supported by {} · {}",
                                c.provider,
                                if c.model.is_empty() {
                                    "(provider default)"
                                } else {
                                    &c.model
                                }
                            ),
                            false,
                        ),
                        None => line(
                            "cache      unknown (gateway probe pending or unavailable)".to_string(),
                            true,
                        ),
                    }
                    if stats.cached_tokens > 0 {
                        line(
                            format!(
                                "cache hits {} tk served from cache this run",
                                crate::ui::chrome::fmt_tokens(stats.cached_tokens)
                            ),
                            false,
                        );
                    } else {
                        line(
                            "cache hits none reported (many local providers never report them)"
                                .to_string(),
                            true,
                        );
                    }
                    if stats.last_input_tokens > 0 {
                        line(
                            format!(
                                "context    {} tk sent on the latest model call",
                                crate::ui::chrome::fmt_tokens(stats.last_input_tokens)
                            ),
                            false,
                        );
                    } else {
                        line("context    no model call observed yet".to_string(), true);
                    }
                    if !stats.effective_model.is_empty() {
                        line(format!("served by  {}", stats.effective_model), false);
                    }
                    line(String::new(), true);
                    line(
                        "the gateway enables prompt caching per run automatically when the"
                            .to_string(),
                        true,
                    );
                    line(
                        "provider supports it (auto = on when available); nothing to configure"
                            .to_string(),
                        true,
                    );
                    draw_rows(rows, usize::MAX, Vec::new())
                },
            ))
            .build()
    });
}

pub fn open_help(cx: Scope, ctx: &UiCtx) {
    let ctx2 = ctx.clone();
    let size = modal_size(
        72,
        (HELP_LINES.len() + crate::commands::HELP_EXTRA.len()) as i32 + 10,
    );
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let mut col = Element::new()
            .style(LayoutStyle::column().gap(1).padding(Edges::all(1)))
            .focusable()
            .autofocus()
            .shortcut(KeyChord::plain(Key::Escape), {
                let ctx = ctx2.clone();
                move |_| ctx.close_modal()
            });
        col = col.child(abstracttui::widgets::Logo::new().element(&t).build());
        let mut body = Element::new().style(LayoutStyle::column());
        let all_lines: Vec<&(&str, &str)> = HELP_LINES
            .iter()
            .chain(std::iter::once(&("", "")))
            .chain(crate::commands::HELP_EXTRA.iter())
            .collect();
        let n_lines = all_lines.len() as i32;
        for (key, desc) in all_lines {
            let key = key.to_string();
            let desc = desc.to_string();
            let (accent, muted) = (t.accent, t.text_muted);
            body = body.child(
                Element::new()
                    .style(LayoutStyle::line(1))
                    .draw(move |canvas, rect| {
                        canvas.print(Point::new(rect.x, rect.y), &key, accent, Rgba::TRANSPARENT);
                        let avail = (rect.right() - rect.x - 18).max(0);
                        let fitted = text::truncate_ellipsis(&desc, avail);
                        canvas.print(
                            Point::new(rect.x + 18, rect.y),
                            &fitted,
                            muted,
                            Rgba::TRANSPARENT,
                        );
                    })
                    .build(),
            );
        }
        // Scrollable: at 80x24 the command list is taller than the modal —
        // without a scroll the tail (incl. newest commands) silently
        // clipped (adversary finding 9).
        col = col.child(
            Scroll::new(body.build())
                .content_size(size.w - 4, n_lines)
                .layout(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)))
                .element(mcx, &t)
                .autofocus()
                .build(),
        );
        col = col.child(hint_row(&t, "↑↓ scroll · Esc closes".into()));
        col.build()
    });
}
