//! Modal surfaces: tool approval, ask-user, pickers, tools, help.
//!
//! All modals are focus-trapped `Modal` overlays; one is open at a time
//! (`UiCtx::open_modal` closes the previous). State created inside a modal
//! lives in the modal's scope and dies on close.

use abstracttui::app::current_viewport;
use abstracttui::prelude::*;
use abstracttui::text;
use abstracttui::widgets::{Button, List, Scroll, TextInput};

use std::rc::Rc;

use crate::commands::HELP_LINES;
use crate::runner::Cmd;
use crate::store::Store;
use crate::transcript::{PendingWait, WaitKind};
use crate::ui::UiCtx;

/// " — gate: X" when a served-disabled row names its gate; empty when it
/// does not. ONE spelling for every modal surface (it was hand-rolled at
/// three sites and each had to carry its own empty-guard).
fn gate_suffix(enable_gate: &str) -> String {
    if enable_gate.is_empty() {
        String::new()
    } else {
        format!(" — gate: {enable_gate}")
    }
}

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

/// Wrap `source` into display lines, stopping early once `cap` lines
/// exist (`None` = wrap everything — the ask modal's lane). Blank
/// source lines are KEPT as paragraph breaks: the ask prompt is real
/// prose and `text::wrap("")` yields nothing, which glued paragraphs
/// together in the old fold.
fn wrap_lines(source: &str, width: i32, cap: Option<usize>) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in source.lines() {
        if raw.trim().is_empty() {
            lines.push(String::new());
        } else {
            lines.extend(text::wrap(raw, width.max(8)));
        }
        if cap.is_some_and(|c| lines.len() > c) {
            break;
        }
    }
    lines
}

/// One pre-wrapped text block drawn line-by-line (rows = exact height,
/// so a Scroll wrapping it gets an honest content_size).
fn lines_view(lines: Vec<String>, ink: Rgba) -> (View, i32) {
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

fn wrapped_lines(source: &str, width: i32, ink: Rgba, cap: usize) -> (View, i32) {
    let mut lines = wrap_lines(source, width, Some(cap));
    if lines.len() > cap {
        lines.truncate(cap);
        // The marker is USER-FACING: name the fact, never point at
        // server internals (operator ruling 2026-07-26: users do not
        // read ledgers). The one remaining caller is the approval
        // modal's `f` JSON view, which IS the fullest in-client view —
        // there is no fuller surface to point at.
        lines.push("… [#TRUNCATION: shortened for display]".into());
    }
    lines_view(lines, ink)
}

// ---------------------------------------------------------------------------
// Tool approval
// ---------------------------------------------------------------------------

/// One pre-wrapped body row: segments drawn left-to-right at fixed
/// columns. Rows are computed ONCE at open (fixed modal width), so the
/// scroll's content height is exact.
#[derive(Clone)]
struct BodyRow {
    segs: Vec<(i32, String, SegInk)>,
}

#[derive(Clone, Copy)]
enum SegInk {
    Accent,
    Text,
    Muted,
    Faint,
    Warn,
}

fn seg_ink(t: &TokenSet, ink: SegInk) -> Rgba {
    match ink {
        SegInk::Accent => t.accent,
        SegInk::Text => t.text,
        SegInk::Muted => t.text_muted,
        SegInk::Faint => t.text_faint,
        SegInk::Warn => t.warn,
    }
}

/// Human-readable body rows for an approval batch (BUG b, 2026-07-22:
/// "the tool itself is not really shown nor the parameters"). Per call:
/// a separator (batches of 2+), a headline (tool name + needed tier), a
/// one-line intent summary, the COMMAND first-class for execute_command,
/// then aligned `key  value` rows. Truncation is honest and points at
/// the `f` full-JSON toggle (a client surface — never the ledger:
/// operator ruling 2026-07-26, users do not read ledgers).
fn approval_body_rows(views: &[crate::ui::approval_view::CallView], width: i32) -> Vec<BodyRow> {
    let width = width.max(24);
    let mut rows: Vec<BodyRow> = Vec::new();
    let total = views.len();
    let mut any_truncated = false;
    for (i, v) in views.iter().enumerate() {
        if total > 1 {
            let label = format!("── call {}/{total} ", i + 1);
            let fill = "─".repeat(((width - label.chars().count() as i32).max(0)) as usize);
            rows.push(BodyRow {
                segs: vec![(0, format!("{label}{fill}"), SegInk::Faint)],
            });
        }
        // Headline: the tool NAME is the thing being approved. The name
        // truncates leaving room for the tier tag; both stay inside the
        // row (draw closures do not clip for us).
        let tier_text = format!("  needs: {}", v.tier.label());
        let tier_cols = tier_text.chars().count() as i32;
        let name_fit =
            text::truncate_ellipsis(&format!("⚙ {}", v.name), (width - tier_cols).max(8));
        // DISPLAY width, not char count (cycle-3 P2-3): a width-2
        // grapheme in a tool name (CJK MCP tools) would overlap the
        // tier segment under a char-count offset.
        let name_cols = text::width(&name_fit);
        rows.push(BodyRow {
            segs: vec![
                (0, name_fit, SegInk::Accent),
                (
                    name_cols,
                    text::truncate_ellipsis(&tier_text, (width - name_cols).max(4)),
                    SegInk::Faint,
                ),
            ],
        });
        // Served-disabled honesty (cycle-2 adversary P2-1): a call whose
        // tool the gateway serves gate-disabled reaches this prompt only
        // through the defense-in-depth lane — the tier line alone would
        // imply normal approvability while the belt clamps it to ask
        // and the gateway will refuse the call. Say so, with the gate.
        if v.served_disabled {
            let gate = gate_suffix(&v.enable_gate);
            rows.push(BodyRow {
                segs: vec![(
                    2,
                    text::truncate_ellipsis(
                        &format!("⚠ disabled on this gateway{gate} (approval cannot run it)"),
                        width - 2,
                    ),
                    SegInk::Warn,
                )],
            });
        }
        if !v.summary.is_empty() {
            rows.push(BodyRow {
                segs: vec![(
                    2,
                    text::truncate_ellipsis(&v.summary, width - 2),
                    SegInk::Muted,
                )],
            });
        }
        if let Some(cmd) = v.command.as_deref() {
            // The command string IS what is approved: full-width, wrapped,
            // capped with an honest note.
            let wrapped = text::wrap(&format!("$ {cmd}"), (width - 2).max(8));
            let cap = 5usize;
            for line in wrapped.iter().take(cap) {
                rows.push(BodyRow {
                    segs: vec![(2, line.clone(), SegInk::Text)],
                });
            }
            if wrapped.len() > cap {
                any_truncated = true;
                rows.push(BodyRow {
                    segs: vec![(
                        2,
                        format!("… (+{} more lines)", wrapped.len() - cap),
                        SegInk::Faint,
                    )],
                });
            }
        }
        if !v.params.is_empty() {
            let key_w = v
                .params
                .iter()
                .map(|(k, _)| k.chars().count())
                .max()
                .unwrap_or(0)
                .min(18) as i32;
            for (k, val) in &v.params {
                let key = text::truncate_ellipsis(k, key_w.max(4));
                let val_x = 2 + key_w + 2;
                rows.push(BodyRow {
                    segs: vec![
                        (2, key, SegInk::Faint),
                        (
                            val_x,
                            text::truncate_ellipsis(val, (width - val_x).max(8)),
                            SegInk::Text,
                        ),
                    ],
                });
            }
        }
        any_truncated |= v.truncated;
    }
    if any_truncated {
        rows.push(BodyRow {
            segs: vec![(
                0,
                "values shortened — f shows the full JSON".to_string(),
                SegInk::Faint,
            )],
        });
    }
    rows
}

fn draw_body_rows(t: &TokenSet, rows: Vec<BodyRow>) -> View {
    let inks: Vec<Vec<(i32, String, Rgba)>> = rows
        .iter()
        .map(|r| {
            r.segs
                .iter()
                .map(|(x, s, ink)| (*x, s.clone(), seg_ink(t, *ink)))
                .collect()
        })
        .collect();
    let h = inks.len().max(1) as i32;
    Element::new()
        .style(LayoutStyle::line(h))
        .draw(move |canvas, rect| {
            for (row_ix, segs) in inks.iter().enumerate() {
                let y = rect.y + row_ix as i32;
                if y >= rect.bottom() {
                    break;
                }
                for (x, s, ink) in segs {
                    canvas.print(Point::new(rect.x + x, y), s, *ink, Rgba::TRANSPARENT);
                }
            }
        })
        .build()
}

/// The tool-approval prompt — deliberately HAND-ROLLED, not the
/// engine's `ChoicePrompt` decision gate. Re-assessed against
/// abstracttui 0.2.9 (which fixed our 0287 body-slot and 0288
/// kitty-fold filings, the original blockers) and still not adopted;
/// the three remaining gaps are engine API holes, filed as
/// first-app/0271:
/// 1. WIDTH — `ChoicePrompt`'s panel width is content-derived
///    (options/prompt≤52/hint; the body is invisible to `measure`,
///    no caller knob): our options land the gate at ≈45 cols while
///    these cards are built for 72 (the 2026-07-22 readability fix)
///    — they would clip under the body's `.clip()` or need
///    pre-wrapping to a width only knowable by mirroring the
///    engine's private arithmetic.
/// 2. THE `f` TOGGLE — the gate has no non-option key vocabulary
///    (unmatched letters die unconsumed inside the focus trap) and
///    its hint row is hardcoded, so `f full JSON` could neither fire
///    nor be advertised.
/// 3. ESC-DEFER VOCABULARY — `ChoiceOutcome::Cancelled` IS a
///    distinct, defer-wirable ending (verified at source), but
///    `dismissable(true)` forcibly renders a "Cancel" button +
///    "Esc cancels" hint (both hardcoded): a mislabeled affordance
///    beside a real Deny option, on the one surface where labels
///    must not lie. And `ChoicePromptHandle` has no host-retire
///    distinct from user-cancel, so `UiCtx`'s replace/auto-close
///    paths would need a side-channel flag to keep "user deferred"
///    (stay closed) apart from "host retired" (reopen later).
///
/// What 0.2.9 DID give us here: the tree-shortcut shifted-letter
/// fold — the Shift+A double registration below is gone.
pub fn open_approval(cx: Scope, store: Store, ctx: &UiCtx, wait: PendingWait) {
    let tool_calls = match &wait.kind {
        WaitKind::Approval { tool_calls } => tool_calls.clone(),
        _ => return,
    };
    let ctx2 = ctx.clone();
    // Prefer the gateway's served tier/approval when the inventory carried
    // it, so the card's "needs:" tier matches the belt's auto decision.
    let classes = store.tool_classes();
    let views = crate::ui::approval_view::build_call_views_with(&tool_calls, &classes);
    // The tier a batch NEEDS is constant per batch (server truth
    // preferred); only the ACCEPTED tier can change while the modal lives.
    let needed = crate::tool_policy::batch_tier_with(&tool_calls, &classes);
    let width_hint = 76;
    let body_rows = approval_body_rows(&views, width_hint - 4);
    // Height budget: panel padding 2 + content padding 2 + title 1 +
    // tier row 1 + gaps 4 + buttons 1 + hint 1 = 12 fixed rows; the body
    // scroll gets the rest, capped so big batches stay on screen.
    let size = modal_size(width_hint, 12 + (body_rows.len() as i32).clamp(2, 18));
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
                    f.mark_wait_tools(approved);
                    if approved {
                        // Execution re-enters gateway-side with no second
                        // `started` record — the client re-arms the tool
                        // clock at the resume it just decided (a refused
                        // resume rolls back via reopen_wait).
                        f.tool_resumed(&run_id);
                    }
                });
                ctx.send(Cmd::Resume {
                    run_id: run_id.clone(),
                    wait_key: wait_key.clone(),
                    // `approved_by: "user"` (R3, c5028): a human click is
                    // ledger-distinguishable from the policy lane's
                    // `approved_by: "policy"` stamp — other session
                    // clients can tell who (or what) spoke.
                    payload: if approved {
                        serde_json::json!({"approved": true, "approved_by": "user"})
                    } else {
                        serde_json::json!({
                            "approved": false,
                            "approved_by": "user",
                            "reason": "Denied by user",
                        })
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
        // "Approve all" (c5028 consolidation): approves this batch AND
        // sets permissions to `all` — the ONE lane (the old ephemeral
        // blanket died with its three holes). Semantics change honestly
        // disclosed at the gesture: the level is per-session persistent
        // and seeds future sessions via the existing global mirror
        // (hazard 1, operator-disclosed), where the blanket died at
        // session end. Pins and served-disabled rows still gate.
        let approve_all = {
            let d = decide.clone();
            let ctx = ctx2.clone();
            move || {
                crate::ui::apply_permissions(store, &ctx, crate::tool_policy::Tier::All);
                d(true);
            }
        };

        let width = size.w - 4;
        // Body: human-readable cards by default; `f` flips to the full
        // JSON. Each mode builds its OWN Scroll with an exact
        // content_size (a shared scroll would let the shorter view
        // scroll into blank space).
        let show_json = mcx.signal(false);
        let json_text = crate::ui::approval_view::full_json(&tool_calls);

        Element::new()
            .style(LayoutStyle::column().gap(1).padding(Edges::all(1)))
            .focusable()
            .autofocus()
            .shortcut(KeyChord::plain(Key::Char('a')), {
                let a = approve.clone();
                move |_| a()
            })
            // Shift+A has TWO wire spellings (live P0, 2026-07-23: "approve
            // all" was a dead key on kitty-protocol terminals — legacy
            // wires bake the shift into the char, byte 0x41 → Char('A');
            // the kitty protocol reports the base identity + modifier,
            // Char('a') + SHIFT). Since abstracttui 0.2.9 (our first-app
            // 0286/0288, fixed at the engine) EVERY chord-match site —
            // tree shortcuts included (`tree.rs` normalizes both the
            // event chord and each registered chord) — folds shifted
            // letters to one canonical form: uppercase char, SHIFT
            // dropped. ONE registration in the canonical spelling now
            // matches both wires; the interim double registration this
            // block carried is deleted. Pinned end-to-end by
            // `approve_all_fires_on_the_kitty_shift_a_spelling_and_covers_the_next_batch`
            // (raw CSI 97;2u bytes through the real parser).
            .shortcut(KeyChord::plain(Key::Char('A')), {
                let aa = approve_all.clone();
                move |_| aa()
            })
            .shortcut(KeyChord::plain(Key::Char('d')), {
                let d = deny.clone();
                move |_| d()
            })
            .shortcut(KeyChord::plain(Key::Char('f')), move |_| {
                show_json.update(|j| *j = !*j);
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
            // Tier honesty line, DYN (fix 3c: a static snapshot went stale
            // when the accepted tier changed while the prompt was up —
            // lowering was never re-rendered). Reading `accepted_tier`
            // reactively re-draws this line on any tier change; `needed`
            // is fixed per batch. (Raising past `needed` also closes the
            // modal via wire_wait_modals; this keeps the line truthful in
            // the frames before that lands, and for lowering, which does
            // not close.)
            .child(dyn_view(LayoutStyle::line(1).shrink(0.0), move || {
                let t2 = abstracttui::app::current_theme().tokens;
                let accepted = crate::tool_policy::Tier::parse_or_default(
                    &store.accepted_tier.get(),
                );
                let line = format!(
                    "permissions: {} — this batch needs: {} · /permissions <read|write|all> changes",
                    accepted.label(),
                    needed.label()
                );
                hint_row(&t2, line)
            }))
            .child(dyn_view_scoped(
                // basis 0: the scroll absorbs ALL flex pressure so the
                // fixed rows (title/buttons/hint) never shrink away.
                LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),
                {
                    let body_rows = body_rows.clone();
                    move |scx| {
                        let t2 = abstracttui::app::current_theme().tokens;
                        let (view, rows) = if show_json.get() {
                            let (v, r) = wrapped_lines(&json_text, width, t2.text_muted, 400);
                            (v, r)
                        } else {
                            let h = body_rows.len().max(1) as i32;
                            (draw_body_rows(&t2, body_rows.clone()), h)
                        };
                        Scroll::new(view)
                            .content_size(width, rows)
                            .layout(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)))
                            .element(scx, &t2)
                            // Focus the body scroll so ↑↓ page long content;
                            // the a/A/d/f/Esc shortcuts live on the root
                            // (still on the dispatch path).
                            .autofocus()
                            .build()
                    }
                },
            ))
            .child(
                Element::new()
                    .style(LayoutStyle::row().h(1).gap(2).shrink(0.0))
                    .child(Button::new("approve (a)").on_click(approve).view(mcx))
                    .child(
                        // The label discloses the semantics change at the
                        // gesture (hazard 1): the old blanket was
                        // session-ephemeral; this sets the persistent
                        // level.
                        Button::new("approve all (A — sets permissions: all)")
                            .on_click(approve_all)
                            .view(mcx),
                    )
                    .child(Button::new("deny (d)").on_click(deny).view(mcx))
                    .build(),
            )
            .child(hint_row(
                &t,
                "a approve · A approve all (sets permissions: all) · d deny · f full JSON · ↑↓ scroll · Esc defers"
                    .into(),
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
    // An ask is the agent talking TO the human — the human cannot answer
    // what they cannot read, so the prompt renders FULL and scrolls when
    // long (operator ruling 2026-07-26). The old shape (fixed 70x13, cap
    // 5 lines + truncation marker) cut the question mid-sentence AND let
    // the input/hint rows clip below the panel bottom — an unanswerable
    // ask.
    //
    // Width first (viewport-clamped), then wrap ONCE at the final text
    // width so the scroll's content height is exact (the approval
    // modal's fixed-width recipe). -5 = panel padding 2 + content
    // padding 2 + the Scroll's reserved scrollbar column.
    let panel_w = modal_size(70, 6).w;
    let text_w = (panel_w - 5).max(8);
    let prompt_lines = wrap_lines(&prompt, text_w, None);
    let prompt_rows = prompt_lines.len().max(1) as i32;
    // Fixed vertical budget: panel padding 2 + content padding 2 +
    // title 1 + input 1 + hint 1 + column gaps 3 = 10 rows. The modal
    // grows with the prompt until modal_size's viewport clamp; past
    // that the prompt region SCROLLS. The title/input/hint rows are
    // declared Cells(1), which the modal floors (engine 0240), and the
    // scroll absorbs all flex pressure (basis 0) — a height squeeze
    // shrinks the question region, never the response affordances.
    const FIXED_ROWS: i32 = 10;
    let size = modal_size(panel_w, FIXED_ROWS + prompt_rows);
    let scrollable = size.h < FIXED_ROWS + prompt_rows;
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let run_id = wait.run_id.clone();
        let wait_key = wait.wait_key.clone();
        let answer = mcx.signal(String::new());
        // Prompt scroll offset. The TextInput KEEPS focus (typing never
        // moves); it leaves ↑↓/PgUp/PgDn unconsumed, so the root
        // shortcuts below drive this signal while the engine Scroll
        // binds the same one — wheel gestures (hit-tested, focus-free)
        // and keys move one offset. The clamp recomputes against the
        // LIVE panel height (the modal re-clamps its bounds on resize
        // from the same size request, `size.min(viewport)`): open-time
        // constants would strand the keyboard short of the tail after
        // a shrink, and "full text, always reachable" is the ruling.
        let scroll = mcx.signal(0i32);
        let scroll_by = move |delta: i32, by_page: bool| {
            let live_h = size.h.min(current_viewport().h);
            let view_rows = (live_h - FIXED_ROWS).max(1);
            let step = if by_page { delta * view_rows } else { delta };
            let max_scroll = (prompt_rows - view_rows).max(0);
            scroll.update(|o| *o = (*o + step).clamp(0, max_scroll));
        };

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

        let (prompt_view, _rows) = lines_view(prompt_lines, t.text);
        // The hint always fits un-ellipsized (the old 88-char hint was
        // cut mid-sentence at the default width — an invisible
        // affordance): the reopen-after-defer teaching lives on the
        // status strip, where it matters.
        let hint = if scrollable {
            "Enter answers · ↑↓/PgDn scroll · Esc defers (run keeps waiting)"
        } else {
            "Enter answers · Esc defers (the run keeps waiting)"
        };
        Element::new()
            .style(LayoutStyle::column().gap(1).padding(Edges::all(1)))
            .child(title_row(&t, "the agent asks".into()))
            .child(
                // Auto-hidden bar: a short ask shows no thumb noise; a
                // long one renders the overflow affordance at the edge.
                Scroll::new(prompt_view)
                    .content_size(text_w, prompt_rows)
                    .offset_y(scroll)
                    .scrollbar_auto_hide(true)
                    .layout(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)))
                    .element(mcx, &t)
                    .build(),
            )
            .child(
                TextInput::new()
                    .value(answer)
                    .placeholder("your answer… (Enter sends)")
                    // The input autofocuses, and the classic
                    // yield-to-caret rule would hide the placeholder —
                    // leaving a bare caret box as the only response
                    // affordance (the 2026-07-26 screenshot's failure
                    // mode). Focused-and-empty keeps the teaching
                    // visible beside the caret.
                    .placeholder_while_focused(true)
                    .on_submit({
                        let send = send.clone();
                        move |text| send(text.to_string())
                    })
                    .layout(LayoutStyle::line(1))
                    .element(mcx, &t)
                    .autofocus()
                    .build(),
            )
            .child(hint_row(&t, hint.into()))
            .shortcut(KeyChord::plain(Key::Up), move |_| scroll_by(-1, false))
            .shortcut(KeyChord::plain(Key::Down), move |_| scroll_by(1, false))
            .shortcut(KeyChord::plain(Key::PageUp), move |_| scroll_by(-1, true))
            .shortcut(KeyChord::plain(Key::PageDown), move |_| scroll_by(1, true))
            .shortcut(KeyChord::plain(Key::Escape), {
                // Esc DEFERS — recorded in `dismissed_wait` exactly like
                // the approval prompt. Leaving the wait pending is
                // legitimate (the run stays durable server-side), but a
                // bare close is NOT enough: the close's epoch bump re-runs
                // `wire_wait_modals`, which sees the still-pending,
                // not-dismissed wait and reopens this prompt in the same
                // flush — Esc was a no-op blink (cycle-2 integration
                // finding P1-2). Enter on the empty composer reopens.
                let ctx = ctx2.clone();
                let step_id = wait.step_id.clone();
                move |_| {
                    *ctx.dismissed_wait.borrow_mut() = Some(step_id.clone());
                    ctx.close_modal();
                }
            })
            .build()
    });
}

// ---------------------------------------------------------------------------
// Pickers
// ---------------------------------------------------------------------------

/// One single-select picker modal: title + `List` (+ optional hint row).
///
/// Activation is the ENGINE's (`List::on_activate`, 0.2.1 — the fix for
/// the 0250 report this app filed): Enter, Space, or a click on the
/// already-selected row runs `on_choose(ix)`; arrow movement only
/// browses. The engine completes ALL of its bookkeeping (selection
/// write, ensure-visible) BEFORE the callback, so `on_choose` may close
/// the modal — or replace it with the next stage — synchronously.
/// `on_choose` owns what happens next (close, or open a follow-up
/// modal); the helper never closes on its behalf.
///
/// Esc runs `on_cancel` (default: plain close); the theme picker uses
/// it to revert its live preview. `on_selection` observes selection
/// MOVEMENT through a tracked effect (the live-preview seam) — never
/// wire commitment to it (the 0250 movement-vs-activation split).
struct Picker {
    title: String,
    labels: Vec<String>,
    /// LIVE row source (reactive-picker follow-up, flow's c5483 thread):
    /// when set, the rows rebuild from the signals this closure reads —
    /// a catalog refresh landing while the picker is OPEN renders
    /// without a reopen (the 13-row parity incident's static-shell
    /// limit, retired; code's on_open live-rendering footer hook is the
    /// named reference pattern). `labels` then serves only the height
    /// arithmetic at open. Choose-side contract: `on_choose` must
    /// RE-READ its source at activation — an index into rebuilt rows
    /// applied to an open-time snapshot would desync.
    live: Option<Rc<dyn Fn() -> Vec<String>>>,
    start: usize,
    /// Caller-computed (each picker's height arithmetic differs).
    size: Size,
    hint: Option<String>,
    on_selection: Option<Box<dyn Fn(usize)>>,
    on_choose: Box<dyn Fn(usize)>,
    on_cancel: Option<Box<dyn Fn()>>,
}

fn open_picker(cx: Scope, ctx: &UiCtx, picker: Picker) {
    let ctx2 = ctx.clone();
    ctx.open_modal(cx, picker.size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let selection = mcx.signal(picker.start);
        if let Some(observe) = picker.on_selection {
            mcx.effect(move || observe(selection.get()));
        }
        // Rc: the live branch rebuilds the List per signal change and
        // each build needs the activate callback.
        let choose: Rc<dyn Fn(usize)> = Rc::from(picker.on_choose);
        let cancel: Box<dyn Fn()> = picker.on_cancel.unwrap_or_else(|| {
            let ctx = ctx2.clone();
            Box::new(move || ctx.close_modal())
        });
        let list_layout = LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0));
        let list: View = match picker.live.clone() {
            Some(rows_of) => {
                let choose = choose.clone();
                dyn_view_scoped(list_layout.clone(), move |lcx| {
                    let t2 = abstracttui::app::current_theme().tokens;
                    let labels = rows_of();
                    // Selection rides the modal-scoped signal across
                    // rebuilds; the engine clamps rendering, and the
                    // choose closures .get(ix) safely — a shrunken
                    // catalog can never activate out of range.
                    let choose = choose.clone();
                    List::new(labels)
                        .selection(selection)
                        .on_activate(move |ix| choose(ix))
                        .layout(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)))
                        .element(lcx, &t2)
                        .autofocus()
                        .build()
                })
            }
            None => {
                let choose = choose.clone();
                List::new(picker.labels)
                    .selection(selection)
                    .on_activate(move |ix| choose(ix))
                    .layout(list_layout)
                    .element(mcx, &t)
                    .autofocus()
                    .build()
            }
        };
        let mut root = Element::new()
            .style(LayoutStyle::column().gap(1).padding(Edges::all(1)))
            .shortcut(KeyChord::plain(Key::Escape), move |_| cancel())
            .child(title_row(&t, picker.title))
            .child(list);
        if let Some(hint) = picker.hint {
            root = root.child(hint_row(&t, hint));
        }
        root.build()
    });
}

pub fn open_theme_picker(cx: Scope, store: Store, ctx: &UiCtx) {
    let themes = abstracttui::theme::themes();
    let labels: Vec<String> = themes
        .iter()
        .map(|th| format!("{}{}", th.label, if th.dark { "" } else { "  (light)" }))
        .collect();
    let original = abstracttui::app::current_theme().id;
    let start = themes.iter().position(|th| th.id == original).unwrap_or(0);
    // 56 wide (POLISH-1/UX-16): the old 44 self-truncated the title's own
    // key hints ("Enter keeps…"); the hints also move to a hint row so
    // the title never has to carry them.
    let size = modal_size(56, (labels.len() as i32 + 8).min(26));
    let choose_ctx = ctx.clone();
    let cancel_ctx = ctx.clone();
    open_picker(
        cx,
        ctx,
        Picker {
            title: "pick a theme".into(),
            labels,
            live: None,
            start,
            size,
            hint: Some("↑↓ previews live · Enter keeps · Esc reverts".into()),
            // Live preview: moving the selection applies the theme
            // immediately (movement observer, never a commitment).
            on_selection: Some(Box::new(|ix| {
                if let Some(th) = abstracttui::theme::themes().get(ix) {
                    abstracttui::app::set_theme_by_id(th.id);
                }
            })),
            on_choose: Box::new(move |ix| {
                if let Some(th) = abstracttui::theme::themes().get(ix) {
                    abstracttui::app::set_theme_by_id(th.id);
                    crate::ui::save_theme_pref(&choose_ctx, th.id);
                    store.notify(format!("theme: {}", th.label));
                }
                choose_ctx.close_modal();
            }),
            // Esc abandons the preview: restore the pre-open theme.
            on_cancel: Some(Box::new(move || {
                abstracttui::app::set_theme_by_id(original);
                cancel_ctx.close_modal();
            })),
        },
    );
}

/// Should this workflow appear in the CODING picker?
///
/// The catalog's agent.v1 set includes entrypoints other lanes own — the
/// entity-conversation flows (summoned via the entity lane, not `/workflow`)
/// and plumbing-test bundles. Listing them here made the picker read like a
/// registry dump (operator finding 2026-08-01). The fold's answer-source
/// recognition keeps the FULL set; this predicate narrows only what a human
/// browses.
fn workflow_pickable(w: &crate::store::Workflow) -> bool {
    if w.bundle_id == "entity-life" {
        return false;
    }
    if w.bundle_id.ends_with("-test") || w.bundle_id.contains("llm-test") {
        return false;
    }
    true
}

/// The description, made READABLE: first sentence only, whitespace collapsed.
/// Bundle descriptions are paragraphs written for catalogs; a picker row
/// needs the one line a human scans.
fn workflow_desc_line(desc: &str, budget: i32) -> String {
    let flat = desc.split_whitespace().collect::<Vec<_>>().join(" ");
    let first = match flat.find(". ") {
        Some(i) => &flat[..i + 1],
        None => flat.as_str(),
    };
    text::truncate_ellipsis(first, budget)
}

pub fn open_workflow_picker(cx: Scope, store: Store, ctx: &UiCtx) {
    let workflows: Vec<crate::store::Workflow> = store
        .workflows
        .get_untracked()
        .into_iter()
        .filter(workflow_pickable)
        .collect();
    if workflows.is_empty() {
        store.notify("no agent workflows discovered yet (is the gateway up?)");
        return;
    }
    let current = store.workflow.get_untracked();
    let labels: Vec<String> = workflows
        .iter()
        .map(|w| workflow_row(w, &current))
        .collect();
    let start = workflows
        .iter()
        .position(|w| w.bundle_id == current.bundle_id && w.flow_id == current.flow_id)
        .unwrap_or(0);
    // Wide + tall: descriptions were the point of the row and 84 cols
    // truncated nearly all of them (operator finding 2026-08-01).
    let size = modal_size(120, (labels.len() as i32 + 7).min(30));
    let choose_ctx = ctx.clone();
    open_picker(
        cx,
        ctx,
        Picker {
            title: "agent workflow — ↑↓ browse · Enter selects · Esc closes".into(),
            labels,
            // LIVE rows: the /workflow dispatch fires LoadCatalog right
            // before this opens — entrypoints registered while the
            // picker is open render in place (the static-shell limit
            // this incident class named, retired).
            live: Some(Rc::new(move || {
                let current = store.workflow.get();
                store.workflows.with(|ws| {
                    ws.iter()
                        .filter(|w| workflow_pickable(w))
                        .map(|w| workflow_row(w, &current))
                        .collect::<Vec<_>>()
                })
            })),
            start,
            size,
            hint: None,
            on_selection: None,
            on_choose: Box::new(move |ix| {
                // Re-read at activation: the rows rebuilt from this
                // signal, so the open-time snapshot may be stale.
                // Index into the FILTERED view — the same predicate the rows
                // used, or a hidden entry would shift every selection below it.
                let picked = store.workflows.with_untracked(|ws| {
                    ws.iter().filter(|w| workflow_pickable(w)).nth(ix).cloned()
                });
                if let Some(w) = picked {
                    let gating_capable = w.supports_gating();
                    store.workflow.set(w.clone());
                    crate::ui::persist_prefs(&choose_ctx, |p| {
                        p.bundle_id = Some(w.bundle_id.clone());
                        p.flow_id = Some(w.flow_id.clone());
                    });
                    store.notify(format!("workflow: {}", w.label()));
                    // Switching workflows resets any gating choice — a mode
                    // chosen for the coder must not silently ride onto a
                    // different workflow (the reasoning coupling rule,
                    // applied to gating).
                    if !gating_capable {
                        store.gating_mode.set(String::new());
                    }
                    if gating_capable {
                        // Stage 2: ask gated vs unattended (the operator's
                        // "open a modal on selecting this workflow"),
                        // synchronous replacement like the model picker's
                        // stages. Default Yes = gated.
                        open_gating_modal(cx, store, &choose_ctx);
                        return;
                    }
                }
                choose_ctx.close_modal();
            }),
            on_cancel: None,
        },
    );
}

/// The gating choice, opened after picking a gating-capable workflow
/// (the multi-agent coder). Yes = gated (the default: the coder pauses
/// for your approval); No = unattended (skips the approval pauses —
/// tools still gate per the permission mode). Session-scoped, never
/// persisted; Esc keeps the current setting.
pub fn open_gating_modal(cx: Scope, store: Store, ctx: &UiCtx) {
    let cur_auto = store.gating_mode.get_untracked() == "auto";
    let labels = vec![
        format!(
            "{}Yes — gated: pause for my approval at each gate (recommended)",
            if cur_auto { "  " } else { "● " }
        ),
        format!(
            "{}No — unattended: run to the end without asking (tools still gate via /permissions)",
            if cur_auto { "● " } else { "  " }
        ),
    ];
    let size = modal_size(76, 8);
    let choose_ctx = ctx.clone();
    open_picker(
        cx,
        ctx,
        Picker {
            title:
                "run gated? — the coder pauses for approval unless you say No · Esc keeps current"
                    .into(),
            labels,
            live: None,
            start: if cur_auto { 1 } else { 0 },
            size,
            hint: None,
            on_selection: None,
            on_choose: Box::new(move |ix| {
                if ix == 1 {
                    store.gating_mode.set("auto".into());
                    store.notify(
                        "gating: auto — the coder runs unattended (no approval pauses). \
                         Tools still gate per /permissions · /gating wait re-gates.",
                    );
                } else {
                    store.gating_mode.set(String::new());
                    store.notify("gating: wait — the coder pauses for your approval (the default)");
                }
                choose_ctx.close_modal();
            }),
            on_cancel: None,
        },
    );
}

/// Provider rows incl. the leading "gateway defaults" entry (shared by
/// the open-time snapshot and the live rebuild — one recipe, no drift).
fn provider_rows(providers: &[crate::store::ProviderInfo], cur_provider: &str) -> Vec<String> {
    let mut labels: Vec<String> = vec![format!(
        "{}gateway defaults (no override — the gateway routes)",
        if cur_provider.is_empty() {
            "● "
        } else {
            "  "
        }
    )];
    for p in providers {
        let marker = if p.name == cur_provider { "● " } else { "  " };
        let count = if p.models.is_empty() {
            String::new()
        } else {
            format!("  ({} models)", p.models.len())
        };
        labels.push(format!("{marker}{}{count}", p.name));
    }
    labels
}

/// One workflow row (shared by the open-time snapshot and the live
/// rebuild — one recipe, no drift).
fn workflow_row(w: &crate::store::Workflow, current: &crate::store::Workflow) -> String {
    let marker = if w.bundle_id == current.bundle_id && w.flow_id == current.flow_id {
        "● "
    } else {
        "  "
    };
    let desc = if w.description.is_empty() {
        String::new()
    } else {
        format!(" — {}", workflow_desc_line(&w.description, 74))
    };
    format!(
        "{marker}{} ({}:{}){desc}",
        w.label(),
        w.bundle_id,
        w.flow_id
    )
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
    let labels: Vec<String> = provider_rows(&providers, &cur_provider);
    let start = if cur_provider.is_empty() {
        0
    } else {
        providers
            .iter()
            .position(|p| p.name == cur_provider)
            .map(|i| i + 1)
            .unwrap_or(0)
    };
    let size = modal_size(64, (labels.len() as i32 + 7).min(26));
    let choose_ctx = ctx.clone();
    open_picker(
        cx,
        ctx,
        Picker {
            title: "provider — ↑↓ browse · Enter opens/selects · Esc closes".into(),
            labels,
            // LIVE rows: provider discovery can land while the picker
            // is open (the boot probe + /model race) — rows follow.
            live: Some(Rc::new(move || {
                let cur = store.provider.get();
                store.providers.with(|ps| provider_rows(ps, &cur))
            })),
            start,
            size,
            hint: None,
            on_selection: None,
            on_choose: Box::new(move |ix| {
                let ctx = &choose_ctx;
                if ix == 0 {
                    apply_route(store, ctx, "", "");
                    ctx.close_modal();
                    return;
                }
                // Re-read at activation (live rows rebuilt from the
                // signal; the open-time snapshot may be stale).
                let Some(p) = store.providers.with_untracked(|ps| ps.get(ix - 1).cloned()) else {
                    ctx.close_modal();
                    return;
                };
                if p.models.is_empty() {
                    apply_route(store, ctx, &p.name, "");
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
                open_model_stage(cx, store, ctx, p);
            }),
            on_cancel: None,
        },
    );
}

fn apply_route(store: Store, ctx: &UiCtx, provider: &str, model: &str) {
    // Coupling rule (first-citizen contract): an effort applies only
    // under the route it was chosen for — a provider/model CHANGE
    // resets the reasoning override (stage 3 re-sets it if the user
    // continues; re-picking the same route keeps it).
    let route_changed =
        store.provider.get_untracked() != provider || store.model.get_untracked() != model;
    store.provider.set(provider.to_string());
    store.model.set(model.to_string());
    if route_changed && !store.reasoning.get_untracked().is_empty() {
        store.reasoning.set(String::new());
    }
    let (p, m) = (provider.to_string(), model.to_string());
    crate::ui::persist_prefs(ctx, move |prefs| {
        prefs.provider = Some(p.clone());
        prefs.model = Some(m.clone());
        if route_changed {
            prefs.reasoning = None;
            prefs.reasoning_provider = None;
            prefs.reasoning_model = None;
        }
    });
    let label = match (provider.is_empty(), model.is_empty()) {
        (true, _) => "gateway defaults".to_string(),
        (false, true) => format!("{provider} (provider default model)"),
        (false, false) => format!("{provider} · {model}"),
    };
    store.notify(format!("route: {label}"));
}

/// Public face of `apply_reasoning` for the `/reasoning <level>` fast
/// path (no modal).
pub fn apply_reasoning_public(store: Store, ctx: &UiCtx, level: &str) {
    apply_reasoning(store, ctx, level);
}

/// Persist + apply a reasoning choice ("" = clear the override). The
/// prefs triple carries the route it was chosen under (pair-coupled
/// load drops it when the route changes offline).
fn apply_reasoning(store: Store, ctx: &UiCtx, level: &str) {
    store.reasoning.set(level.to_string());
    let (lv, p, m) = (
        level.to_string(),
        store.provider.get_untracked(),
        store.model.get_untracked(),
    );
    crate::ui::persist_prefs(ctx, move |prefs| {
        if lv.is_empty() {
            prefs.reasoning = None;
            prefs.reasoning_provider = None;
            prefs.reasoning_model = None;
        } else {
            prefs.reasoning = Some(lv.clone());
            prefs.reasoning_provider = Some(p.clone());
            prefs.reasoning_model = Some(m.clone());
        }
    });
    store.notify(if level.is_empty() {
        "reasoning: gateway default (no override)".to_string()
    } else {
        format!("reasoning: {level}")
    });
}

/// Stage 3 — reasoning effort (the first-citizen third axis). Rows are
/// LIVE (the capability probe lands while open). Three-state coupling
/// (contract v1): declared reasoning model → the dial; declared
/// non-reasoning → locked line with a labeled set-anyway override
/// (until core ships match provenance, a hard lock could be fabricated
/// from the registry's default-false row for unknown local models —
/// the worse failure); probe failed/unknown → same override shape,
/// labeled "capability unknown".
pub fn open_reasoning_stage(cx: Scope, store: Store, ctx: &UiCtx) {
    let provider = store.provider.get_untracked();
    let model = store.model.get_untracked();
    store.reasoning_probe.set(None);
    if !model.is_empty() {
        ctx.send(crate::runner::Cmd::ProbeModelReasoning {
            provider: provider.clone(),
            model: model.clone(),
        });
    }
    let target = if model.is_empty() {
        format!("{} (provider default model)", provider)
    } else {
        model.clone()
    };
    let rows_store = store;
    let labels = reasoning_rows(rows_store);
    let size = modal_size(70, 14);
    let choose_ctx = ctx.clone();
    open_picker(
        cx,
        ctx,
        Picker {
            title: format!("reasoning — {target} · Enter selects · Esc keeps current"),
            labels,
            live: Some(Rc::new(move || reasoning_rows(rows_store))),
            start: 0,
            size,
            hint: None,
            on_selection: None,
            on_choose: Box::new(move |ix| {
                let rows = reasoning_choices(rows_store);
                // Caption rows are non-actionable: no-op, stay open.
                if let Some(Some(level)) = rows.get(ix).map(|c| c.as_deref()) {
                    apply_reasoning(rows_store, &choose_ctx, level);
                    choose_ctx.close_modal();
                }
            }),
            on_cancel: None,
        },
    );
}

/// The stage-3 row LABELS (render) — must stay index-aligned with
/// `reasoning_choices` (dispatch): one source of row ORDER, two
/// projections.
fn reasoning_rows(store: Store) -> Vec<String> {
    build_reasoning_rows(store)
        .into_iter()
        .map(|r| r.0)
        .collect()
}

/// The stage-3 row CHOICES: `Some(level)` applies ("" clears), `None`
/// is a non-actionable caption row.
fn reasoning_choices(store: Store) -> Vec<Option<String>> {
    build_reasoning_rows(store)
        .into_iter()
        .map(|r| r.1)
        .collect()
}

fn build_reasoning_rows(store: Store) -> Vec<(String, Option<String>)> {
    const LADDER: &[&str] = &["low", "medium", "high"];
    let cur = store.reasoning.get();
    let probe = store.reasoning_probe.get();
    let model = store.model.get();
    let mark = |lv: &str| if cur == lv { "● " } else { "  " };
    let mut rows: Vec<(String, Option<String>)> = vec![(
        format!(
            "{}gateway default (no override)",
            if cur.is_empty() { "● " } else { "  " }
        ),
        Some(String::new()),
    )];
    let (caption, offer_levels): (String, Vec<String>) = match &probe {
        None if !model.is_empty() => (
            "capability: probing…".into(),
            LADDER.iter().map(|s| s.to_string()).collect(),
        ),
        Some(p) if p.supported == Some(true) => {
            let levels = if p.levels.is_empty() {
                LADDER.iter().map(|s| s.to_string()).collect()
            } else {
                p.levels.clone()
            };
            ("reasoning model — pick the effort".into(), levels)
        }
        Some(p) if p.supported == Some(false) => (
            "registry: does not reason · below = set anyway".into(),
            LADDER.iter().map(|s| s.to_string()).collect(),
        ),
        _ => (
            "capability unknown · below = set anyway (best-effort)".into(),
            LADDER.iter().map(|s| s.to_string()).collect(),
        ),
    };
    rows.push((format!("{}none", mark("none")), Some("none".into())));
    rows.push((format!("· {caption}"), None));
    for lv in &offer_levels {
        rows.push((format!("{}{lv}", mark(lv)), Some(lv.clone())));
    }
    rows
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
    let size = modal_size(70, (labels.len() as i32 + 7).min(28));
    let choose_ctx = ctx.clone();
    open_picker(
        cx,
        ctx,
        Picker {
            title: format!(
                "{} models — ↑↓ browse · Enter selects · Esc closes",
                provider.name
            ),
            labels,
            live: None,
            start,
            size,
            hint: None,
            on_selection: None,
            on_choose: Box::new(move |ix| {
                if ix == 0 {
                    apply_route(store, &choose_ctx, &provider.name, "");
                } else if let Some(m) = provider.models.get(ix - 1) {
                    apply_route(store, &choose_ctx, &provider.name, m);
                }
                // Stage 3 — the reasoning dial (first-citizen third
                // axis): synchronous stage replacement, same contract
                // as stage 1 → 2. Esc there keeps the route just
                // applied and the pre-existing reasoning state.
                open_reasoning_stage(cx, store, &choose_ctx);
            }),
            on_cancel: None,
        },
    );
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
                        // "All off" scopes to the GRANTABLE rows: served-
                        // disabled names are a server fact, not a client
                        // selection — parking them in the user's disabled
                        // set would persist stale names past a gate flip.
                        tools
                            .iter()
                            .filter(|x| !x.served_disabled)
                            .map(|x| x.name.clone())
                            .collect()
                    };
                } else {
                    let ix = cursor.get_untracked();
                    let Some(tool) = tools.get(ix) else { return };
                    // Served-disabled rows are visible, never grantable
                    // (full-catalog surfacing, this seat's c4555
                    // commitment): a toggle teaches the gate instead of
                    // mutating a selection the gateway cannot honor.
                    if tool.served_disabled {
                        let gate = gate_suffix(&tool.enable_gate);
                        store.notify(format!(
                            "{} is disabled on this gateway{gate}",
                            tool.name
                        ));
                        return;
                    }
                    if let Some(pos) = disabled.iter().position(|d| *d == tool.name) {
                        disabled.remove(pos);
                    } else {
                        disabled.push(tool.name.clone());
                    }
                }
                store.disabled_tools.set(disabled.clone());
                crate::ui::persist_tool_prefs(store, &ctx, |p| p.disabled_tools = disabled.clone());
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

        // Per-CATEGORY toggle (operator ask): flip EVERY grantable tool in
        // the cursor's toolset on/off in one keystroke. Toggle semantics
        // by current state — if any grantable tool in the category is ON,
        // turn the whole category OFF; else turn it all ON. Served-disabled
        // rows (a gateway gate, not a client selection) are skipped, so a
        // category that is entirely gate-disabled reports the gate instead
        // of silently doing nothing. Pure app logic over `disabled_tools`
        // (the modal owns the set) — no engine capability needed.
        let toggle_category = {
            let ctx = ctx2.clone();
            move || {
                let tools = store.tools.get_untracked();
                let ix = cursor.get_untracked();
                let Some(here) = tools.get(ix) else { return };
                let toolset = here.toolset.clone();
                let label = if toolset.is_empty() {
                    "other".to_string()
                } else {
                    toolset.clone()
                };
                let in_cat: Vec<&crate::store::ToolInfo> =
                    tools.iter().filter(|t| t.toolset == toolset).collect();
                let grantable: Vec<&&crate::store::ToolInfo> =
                    in_cat.iter().filter(|t| !t.served_disabled).collect();
                if grantable.is_empty() {
                    let gated = in_cat.len();
                    store.notify(format!(
                        "'{label}': all {gated} tool(s) are disabled on this gateway — nothing to toggle"
                    ));
                    return;
                }
                // Prune stale names (tools no longer served) on every edit —
                // same discipline as the single-tool toggle.
                let mut disabled: Vec<String> = store
                    .disabled_tools
                    .get_untracked()
                    .into_iter()
                    .filter(|d| tools.iter().any(|tl| tl.name == *d))
                    .collect();
                let any_on = grantable
                    .iter()
                    .any(|t| !disabled.contains(&t.name));
                // any_on → turn the category OFF; all off → turn it ON.
                for t in &grantable {
                    let is_disabled = disabled.contains(&t.name);
                    if any_on && !is_disabled {
                        disabled.push(t.name.clone());
                    } else if !any_on && is_disabled {
                        disabled.retain(|d| *d != t.name);
                    }
                }
                store.disabled_tools.set(disabled.clone());
                crate::ui::persist_tool_prefs(store, &ctx, |p| p.disabled_tools = disabled.clone());
                store.notify(format!(
                    "'{label}': {} tool(s) turned {}",
                    grantable.len(),
                    if any_on { "OFF" } else { "ON" }
                ));
            }
        };

        // Per-tool approval PIN (item 4): cycle the tool under the cursor
        // none → auto → ask → none. A pin beats the tier in BOTH
        // directions ('auto' auto-approves even above the tier; 'ask'
        // always prompts even for a read) and is persisted alongside the
        // tier. The run-start expansion reads the same pins, so a pin
        // reaches both the server-side policy and the client-side belt.
        let cycle_pin = {
            let ctx = ctx2.clone();
            move || {
                let tools = store.tools.get_untracked();
                let ix = cursor.get_untracked();
                let Some(tool) = tools.get(ix) else { return };
                // No pins on served-disabled rows: an approval decision
                // for a tool the gateway refuses to run is fiction (and
                // the policy layer clamps them to ask regardless).
                if tool.served_disabled {
                    store.notify(format!(
                        "{} is disabled on this gateway — no approval pin to set",
                        tool.name
                    ));
                    return;
                }
                let name = tool.name.clone();
                let mut overrides = store.tool_overrides.get_untracked();
                let pos = overrides.iter().position(|(n, _)| *n == name);
                let next = match pos.map(|p| overrides[p].1.as_str()) {
                    None => Some("auto"),
                    Some("auto") => Some("ask"),
                    _ => None, // "ask" (or stray) → clear
                };
                match (pos, next) {
                    (Some(p), None) => {
                        overrides.remove(p);
                    }
                    (Some(p), Some(d)) => overrides[p].1 = d.to_string(),
                    (None, Some(d)) => overrides.push((name.clone(), d.to_string())),
                    (None, None) => {}
                }
                store.tool_overrides.set(overrides.clone());
                crate::ui::persist_tool_prefs(store, &ctx, |p| p.tool_overrides = overrides.clone());
                store.notify(match next {
                    Some("auto") => format!("{name}: pinned auto (always approves)"),
                    Some("ask") => format!("{name}: pinned ask (always prompts)"),
                    _ => format!("{name}: pin cleared (tier decides)"),
                });
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
            .shortcut(KeyChord::plain(Key::Char('c')), {
                let tc = toggle_category.clone();
                move |_| tc()
            })
            .shortcut(KeyChord::plain(Key::Char('p')), {
                let cp = cycle_pin.clone();
                move |_| cp()
            })
            .shortcut(KeyChord::plain(Key::Char('t')), {
                // Cycle the PERSISTED permissions level (read → write →
                // all → read): the same knob as `/permissions <t>` —
                // one apply path, two surfaces.
                let ctx = ctx2.clone();
                move |_| crate::ui::cycle_permissions(store, &ctx)
            })
            .child(dyn_view(LayoutStyle::line(1), move || {
                let t2 = abstracttui::app::current_theme().tokens;
                let err = store.tools_error.get();
                let tier =
                    crate::tool_policy::Tier::parse_or_default(&store.accepted_tier.get());
                // Count only disabled names that EXIST in the inventory:
                // stale names from another gateway must never skew the
                // arithmetic (a raw `n - off` underflowed to u64::MAX in
                // release — adversary finding 2). Served-disabled rows
                // (gateway gate, not user selection) count SEPARATELY —
                // they are visible but never part of the grantable
                // arithmetic.
                let (n, off, gated) = store.tools.with(|tl| {
                    let gated = tl.iter().filter(|t| t.served_disabled).count();
                    // The ONE shared effective-disabled predicate (the
                    // run-start "customized?" decision reads the same
                    // helper — cycle-3 P2-2: two textual copies drift).
                    let off = store
                        .disabled_tools
                        .with(|d| crate::store::Store::effective_user_disabled(tl, d));
                    (tl.len() - gated, off, gated)
                });
                let gated_part = if gated > 0 {
                    format!(" · {gated} gated off server-side")
                } else {
                    String::new()
                };
                let title = if !err.is_empty() {
                    format!("gateway tools — discovery failed: {err}")
                } else if n == 0 && gated == 0 {
                    format!("gateway tools — loading… · permissions: {}", tier.label())
                } else if off == 0 {
                    // "All checked" ≠ the workflow's own pin: the flow's
                    // baked tool set decides when the client sends nothing.
                    format!(
                        "gateway tools — {n} available{gated_part} · untouched (workflow's own tool set decides) · permissions: {}",
                        tier.label()
                    )
                } else {
                    format!(
                        "gateway tools — {} on / {off} off{gated_part} · explicit allowlist · permissions: {}",
                        n.saturating_sub(off),
                        tier.label()
                    )
                };
                title_row(&t2, title)
            }))
            .child(dyn_view(
                LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),
                move || {
                    let tools = store.tools.get();
                    let disabled = store.disabled_tools.get();
                    let overrides = store.tool_overrides.get();
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
                        // Served-disabled rows (full-catalog surfacing):
                        // visible with their gate, no checkbox — the row
                        // is a server fact, not a grantable selection.
                        // Still cursor-reachable so Space/p teach the
                        // gate through the refusal notice.
                        if tool.served_disabled {
                            let gate = gate_suffix(&tool.enable_gate);
                            selectable.push(rows.len());
                            rows.push(RowSpec {
                                text: format!(
                                    "{}  [disabled on this gateway{gate}]",
                                    tool.name
                                ),
                                header: false,
                                checked: None,
                                dim: true,
                            });
                            continue;
                        }
                        let on = !disabled.contains(&tool.name);
                        // Pin marker (item 4): a pinned tool shows its
                        // decision so the override is legible at a glance.
                        let pin = overrides
                            .iter()
                            .find(|(n, _)| *n == tool.name)
                            .map(|(_, d)| match d.as_str() {
                                "auto" => "  [pin:auto]",
                                "ask" => "  [pin:ask]",
                                _ => "",
                            })
                            .unwrap_or("");
                        selectable.push(rows.len());
                        rows.push(RowSpec {
                            text: format!("{}{pin}  {}", tool.name, tool.description),
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
                "↑↓ move · Space toggles · c toggles category · a all on · n all off · p pins auto/ask · t cycles permissions · Enter/Esc closes"
                    .into(),
            ))
            .child(hint_row(
                &t,
                "sticky per session · untouched = workflow defaults; customized = checked set is the run's exact tools · a pin beats the tier"
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
    // Height: padding 2 + title 1 + hint 1 + inter-child gaps 2 = 6 fixed
    // rows; every session needs its own line on top of that.
    let size = modal_size(84, (labels.len() as i32 + 8).min(22));
    let choose_ctx = ctx.clone();
    open_picker(
        cx,
        ctx,
        Picker {
            title: "sessions — ↑↓ browse · Enter continues · Esc closes".into(),
            labels,
            live: None,
            start,
            size,
            hint: Some(
                "memory is durable on the gateway; switching reattaches to a live run if one exists"
                    .into(),
            ),
            on_selection: None,
            on_choose: Box::new(move |ix| {
                if let Some(e) = entries.get(ix) {
                    crate::ui::switch_session(store, &choose_ctx, &e.id);
                }
                choose_ctx.close_modal();
            }),
            on_cancel: None,
        },
    );
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
                    let info = store.mcp_info.get();
                    let mut rows = Vec::new();
                    // Honesty header: WHERE the registry is declared (a
                    // file on the GATEWAY HOST, not this machine) and that
                    // reachability is NOT probed — the gateway does not
                    // probe, and a client-side probe would lie
                    // (client-reachable ≠ gateway-reachable).
                    rows.push(RowSpec {
                        text: crate::entities::mcp_honesty_line(&info, servers.len()),
                        header: false,
                        checked: None,
                        dim: true,
                    });
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
                            // The gateway's empty-state warning carries a
                            // registry recipe: prose stays wrapped, the
                            // JSON-ish recipe renders as an indented block
                            // (screen-text selection copies it).
                            for line in crate::entities::format_mcp_note(&note) {
                                if line.starts_with(' ') {
                                    rows.push(RowSpec {
                                        text: line,
                                        header: false,
                                        checked: None,
                                        dim: true,
                                    });
                                } else {
                                    for wrapped in text::wrap(&line, 78) {
                                        rows.push(RowSpec {
                                            text: wrapped,
                                            header: false,
                                            checked: None,
                                            dim: true,
                                        });
                                    }
                                }
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
/// `/status` — the run/session status card (visibility review P2-5):
/// the boot identity facts (`status_card_rows` — built "for a future
/// /status", now claimed), the CLIENT view (phase, run id, outcome),
/// and the SERVER-TRUTH probe (`get_run` fired at dispatch; renders
/// when it lands). The one place client phase vs gateway run status
/// divergence is inspectable — wrapper roots legitimately stay
/// `waiting` server-side after the client concluded the turn.
pub fn open_status(cx: Scope, store: Store, ctx: &UiCtx) {
    let ctx2 = ctx.clone();
    let gateway_label = ctx.gateway_label.clone();
    let workspace_root = ctx.workspace_root.clone().unwrap_or_default();
    // Sized to the full row set (identity card ~9 + client/server ~6 +
    // chrome): the row renderer windows overflow honestly, but a status
    // card that hides its own point (the client-vs-server rows) behind
    // "↓ N more" defeats the command.
    let size = modal_size(78, 23);
    ctx.open_modal(cx, size, move |_mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let gateway_label = gateway_label.clone();
        let workspace_root = workspace_root.clone();
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
            .child(title_row(&t, "status · Esc closes".into()))
            .child(dyn_view(
                LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),
                move || {
                    let mut rows: Vec<RowSpec> = Vec::new();
                    let mut line = |text: String, dim: bool| {
                        rows.push(RowSpec {
                            text,
                            header: false,
                            checked: None,
                            dim,
                        })
                    };
                    for (label, value) in crate::ui::transcript_view::status_card_rows(
                        store,
                        &gateway_label,
                        &workspace_root,
                    ) {
                        line(format!("{label:<10} {value}"), false);
                    }
                    // The client's view of the run.
                    let phase = match store.phase.get() {
                        crate::store::Phase::Idle => "idle",
                        crate::store::Phase::Starting => "starting",
                        crate::store::Phase::Running => "running",
                    };
                    let run_id = store.run_id.get();
                    line(String::new(), true);
                    line(
                        format!(
                            "client     {phase}{}",
                            if store.paused.get() { " (paused)" } else { "" }
                        ),
                        false,
                    );
                    if !run_id.is_empty() {
                        line(format!("run        {run_id}"), false);
                    }
                    let done = store.fold.with(|f| f.done_note.clone());
                    if !done.is_empty() {
                        line(format!("last run   {done}"), false);
                    }
                    // Server truth: the probe fired at dispatch; a
                    // wrapper root staying `waiting` here while the
                    // client shows idle is the DOCUMENTED divergence
                    // (the composer release is a client decision).
                    match store.run_status_probe.get() {
                        Some((rid, status)) => {
                            let short = rid.get(..8).unwrap_or(&rid);
                            line(format!("gateway    {status} (run {short}…)"), false);
                        }
                        None if !run_id.is_empty() => {
                            line("gateway    probing…".to_string(), true)
                        }
                        None => {}
                    }
                    draw_rows(rows, usize::MAX, Vec::new())
                },
            ))
            .child(hint_row(
                &t,
                "server truth: a wrapper root can stay `waiting` after your turn concluded — it finalizes gateway-side".into(),
            ))
            .build()
    });
}

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
                            "cache hits not reported by this provider — the split below is \
                             derived client-side"
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
                        // NEW vs CARRIED. The number an operator can act on: a prompt is
                        // only expensive to the extent it is NEW, because the carried
                        // prefix is what any prefix cache can reuse. Providers that never
                        // report `cached_input_tokens` still cannot hide this — it is the
                        // difference between two numbers the client already has.
                        if stats.prev_input_tokens > 0 {
                            let last = stats.last_input_tokens;
                            let prev = stats.prev_input_tokens;
                            if last >= prev {
                                let new_tk = last - prev;
                                line(
                                    format!(
                                        "           {} new since the previous call, {} carried \
                                         forward (reusable prefix)",
                                        crate::ui::chrome::fmt_tokens(new_tk),
                                        crate::ui::chrome::fmt_tokens(prev)
                                    ),
                                    true,
                                );
                            } else {
                                line(
                                    format!(
                                        "           {} smaller than the previous call — the \
                                         context was compacted or reset",
                                        crate::ui::chrome::fmt_tokens(prev - last)
                                    ),
                                    true,
                                );
                            }
                        }
                    } else {
                        line("context    no model call observed yet".to_string(), true);
                    }
                    // RE-SEND AMPLIFICATION. An agent loop re-sends its whole transcript
                    // every cycle, so total input dwarfs total output and grows with the
                    // number of cycles. This ratio is the honest cost of the loop, and it
                    // is the thing a cache is there to blunt.
                    if stats.llm_calls > 0 && stats.output_tokens > 0 {
                        line(
                            format!(
                                "run cost   {} tk in / {} tk out over {} model call{} — {:.1}x \
                                 sent per token produced",
                                crate::ui::chrome::fmt_tokens(stats.input_tokens),
                                crate::ui::chrome::fmt_tokens(stats.output_tokens),
                                stats.llm_calls,
                                if stats.llm_calls == 1 { "" } else { "s" },
                                stats.input_tokens as f64 / stats.output_tokens as f64
                            ),
                            false,
                        );
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

// ---------------------------------------------------------------------------
// Workspace (root / access mode / allowed paths)
// ---------------------------------------------------------------------------

/// The gateway's REAL access-mode vocabulary (source: abstractgateway
/// `routes/gateway.py` `_VALID_WORKSPACE_ACCESS_MODES` +
/// `allowed_access_modes`). "" = server-managed: send nothing.
/// `all_except_ignored` is honored only when the gateway trusts client
/// scope (`ABSTRACTGATEWAY_ALLOW_CLIENT_WORKSPACE_SCOPE` / local tool
/// mode) — otherwise the server clamps it to `workspace_only`.
const WORKSPACE_MODES: &[(&str, &str)] = &[
    (
        "",
        "server-managed default — send no mode; the gateway decides",
    ),
    ("workspace_only", "tools stay under the workspace root"),
    (
        "workspace_or_allowed",
        "workspace root + the allowed paths listed below",
    ),
    (
        "all_except_ignored",
        "any absolute path — needs gateway trust in client scope",
    ),
];

/// Normalize an allowed-path entry, or refuse it honestly (bug (d),
/// 2026-07-22). The gateway resolves these paths on ITS OWN host, so:
/// - a leading `~` / `~/` expands against the client `home` (a bare `~`
///   is the home; `~user` cannot be resolved for another account →
///   refuse rather than send a literal `~user` the gateway won't expand);
/// - trailing slashes are stripped (so `/srv/data/` and `/srv/data` are
///   ONE entry, not two — the duplicate-add guard keys on the normal
///   form) while the root `/` is preserved;
/// - a still-relative result is REFUSED: relative to the client cwd is
///   meaningless on the gateway host, and silently sending it invites the
///   red "escapes workspace_root" refusals the modal exists to prevent.
///
/// Pure + `home`-injected so it is testable without touching `$HOME`.
fn normalize_allowed_path(raw: &str, home: &std::path::Path) -> Result<String, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("empty path".into());
    }
    // Tilde expansion (client-side — the gateway does not run a shell).
    let expanded: String = if s == "~" {
        home.display().to_string()
    } else if let Some(rest) = s.strip_prefix("~/") {
        home.join(rest).display().to_string()
    } else if s.starts_with('~') {
        return Err(format!(
            "cannot expand {s:?} — another user's home is unknown; use an absolute path"
        ));
    } else {
        s.to_string()
    };
    if !expanded.starts_with('/') {
        return Err(format!(
            "{s:?} is not absolute — allowed paths resolve on the gateway host, \
             so a relative path is ambiguous; use an absolute path"
        ));
    }
    // Strip trailing slashes but keep the root "/".
    let trimmed = expanded.trim_end_matches('/');
    Ok(if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    })
}

/// `/workspace` — inspect + edit where the agent's tools may touch the
/// filesystem. Root comes from `--workspace` (or the cwd) at boot; mode
/// and allowed paths persist in prefs.json and ride each run as
/// `workspace_access_mode` plus `workspace_allowed_paths`. The red
/// "Path escapes workspace_root" refusals in the transcript are the
/// RUNTIME enforcing this scope — see docs/troubleshooting.md.
pub fn open_workspace(cx: Scope, store: Store, ctx: &UiCtx) {
    let ctx2 = ctx.clone();
    let root_label = ctx
        .workspace_root
        .clone()
        .filter(|r| !r.trim().is_empty())
        .unwrap_or_else(|| "(none sent — the gateway's own workspace)".into());
    let size = modal_size(84, 22);
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let cursor = mcx.signal(0usize);
        let path_draft = mcx.signal(String::new());

        let selectable_count =
            move || WORKSPACE_MODES.len() + store.workspace_allowed.with_untracked(|a| a.len());

        // Space: select the mode under the cursor, or remove the allowed
        // path under it. Both persist immediately (prefs.json is the
        // headless config surface — exec reads the same file).
        let activate = {
            let ctx = ctx2.clone();
            move || {
                let ix = cursor.get_untracked();
                if ix < WORKSPACE_MODES.len() {
                    let mode = WORKSPACE_MODES[ix].0.to_string();
                    store.workspace_mode.set(mode.clone());
                    crate::ui::persist_prefs(&ctx, |p| {
                        p.workspace_mode = if mode.is_empty() {
                            None
                        } else {
                            Some(mode.clone())
                        };
                    });
                    store.notify(if mode.is_empty() {
                        "workspace mode: server-managed (nothing sent)".to_string()
                    } else {
                        format!("workspace mode: {mode}")
                    });
                } else {
                    let pix = ix - WORKSPACE_MODES.len();
                    let mut allowed = store.workspace_allowed.get_untracked();
                    if pix < allowed.len() {
                        let removed = allowed.remove(pix);
                        store.workspace_allowed.set(allowed.clone());
                        crate::ui::persist_prefs(&ctx, |p| p.workspace_allowed = allowed.clone());
                        store.notify(format!("allowed path removed: {removed}"));
                        // Keep the cursor inside the shrunken list.
                        cursor.update(|c| *c = (*c).min(selectable_count().saturating_sub(1)));
                    }
                }
            }
        };
        let move_cursor = move |delta: i64| {
            let n = selectable_count();
            if n == 0 {
                return;
            }
            cursor.update(|c| {
                let cur = *c as i64 + delta;
                *c = cur.clamp(0, n as i64 - 1) as usize;
            });
        };
        let add_path = {
            let ctx = ctx2.clone();
            move |text: &str| {
                // Normalize (or refuse) BEFORE touching state: ~ expands,
                // trailing slashes drop, relative paths are refused
                // honestly — so the dedup + wire form are canonical.
                let p = match normalize_allowed_path(text, &crate::config::home_dir()) {
                    Ok(p) => p,
                    Err(why) => {
                        store.notify(format!("path not added: {why}"));
                        return;
                    }
                };
                let mut allowed = store.workspace_allowed.get_untracked();
                if allowed.contains(&p) {
                    store.notify(format!("{p} is already in the allowed list"));
                    return;
                }
                allowed.push(p.clone());
                store.workspace_allowed.set(allowed.clone());
                // Allowed paths only function in workspace_or_allowed mode
                // (gateway `_effective_workspace_scope` mounts them there
                // alone): auto-pick it when the current mode cannot use
                // them — visible + reversible right here in the modal.
                let mode = store.workspace_mode.get_untracked();
                let switch = mode.is_empty() || mode == "workspace_only";
                if switch {
                    let was = if mode.is_empty() {
                        "server-managed".to_string()
                    } else {
                        mode.clone()
                    };
                    store.workspace_mode.set("workspace_or_allowed".into());
                    store.notify(format!(
                        "added {p}; switched access mode to workspace_or_allowed \
                         (was {was}) so the allowed path takes effect"
                    ));
                } else {
                    store.notify(format!("added allowed path: {p}"));
                }
                crate::ui::persist_prefs(&ctx, |prefs| {
                    prefs.workspace_allowed = allowed.clone();
                    if switch {
                        prefs.workspace_mode = Some("workspace_or_allowed".into());
                    }
                });
            }
        };

        let root_row = {
            let root_label = root_label.clone();
            let (muted, faint) = (t.text_muted, t.text_faint);
            Element::new()
                .style(LayoutStyle::line(1).shrink(0.0))
                .draw(move |canvas, rect| {
                    canvas.print(Point::new(rect.x, rect.y), "root", faint, Rgba::TRANSPARENT);
                    let fitted = text::truncate_ellipsis(&root_label, (rect.w - 7).max(8));
                    canvas.print(
                        Point::new(rect.x + 6, rect.y),
                        &fitted,
                        muted,
                        Rgba::TRANSPARENT,
                    );
                })
                .build()
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
            .shortcut(KeyChord::plain(Key::Char(' ')), {
                let act = activate.clone();
                move |_| act()
            })
            .child(dyn_view(LayoutStyle::line(1), move || {
                let t2 = abstracttui::app::current_theme().tokens;
                let mode = store.workspace_mode.get();
                let n = store.workspace_allowed.with(|a| a.len());
                title_row(
                    &t2,
                    format!(
                        "workspace — mode: {} · {n} allowed path(s) · Esc closes",
                        if mode.is_empty() {
                            "server-managed"
                        } else {
                            &mode
                        }
                    ),
                )
            }))
            .child(root_row)
            .child(dyn_view(
                LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),
                move || {
                    let mode = store.workspace_mode.get();
                    let allowed = store.workspace_allowed.get();
                    let cur = cursor.get();
                    let mut rows = Vec::new();
                    let mut selectable = Vec::new();
                    rows.push(RowSpec {
                        text: "access mode — Space selects".into(),
                        header: true,
                        checked: None,
                        dim: false,
                    });
                    for (id, desc) in WORKSPACE_MODES {
                        selectable.push(rows.len());
                        let label = if id.is_empty() {
                            desc.to_string()
                        } else {
                            format!("{id} — {desc}")
                        };
                        rows.push(RowSpec {
                            text: label,
                            header: false,
                            checked: Some(mode.as_str() == *id),
                            dim: false,
                        });
                    }
                    rows.push(RowSpec {
                        text: "allowed paths (used by workspace_or_allowed) — Space removes".into(),
                        header: true,
                        checked: None,
                        dim: false,
                    });
                    if allowed.is_empty() {
                        rows.push(RowSpec {
                            text: "(none — type a path below, Enter adds)".into(),
                            header: false,
                            checked: None,
                            dim: true,
                        });
                    } else {
                        for p in &allowed {
                            selectable.push(rows.len());
                            rows.push(RowSpec {
                                text: p.clone(),
                                header: false,
                                checked: None,
                                dim: false,
                            });
                        }
                    }
                    let cur = cur.min(selectable.len().saturating_sub(1));
                    draw_rows(rows, cur, selectable)
                },
            ))
            .child(
                TextInput::new()
                    .value(path_draft)
                    .placeholder("add an allowed path (absolute is safest) — Enter adds")
                    .on_submit({
                        let add = add_path.clone();
                        move |text| {
                            add(text);
                            path_draft.set(String::new());
                        }
                    })
                    .layout(LayoutStyle::line(1).shrink(0.0))
                    .element(mcx, &t)
                    .build(),
            )
            .child(hint_row(
                &t,
                "↑↓ move · Space selects mode / removes path · Tab reaches the input · Esc closes"
                    .into(),
            ))
            .child(hint_row(
                &t,
                "the GATEWAY enforces workspace policy — server settings may clamp client paths"
                    .into(),
            ))
            .build()
    });
}

/// One rendered help row: the key (first physical row only) + one
/// wrapped slice of its description. Pre-computed at open so the
/// Scroll's content height is EXACT (the fold/scroll math never guesses).
struct HelpRow {
    key: String,
    desc: String,
}

/// Wrap every help entry's description to `desc_w`, key beside the first
/// slice, continuations indented into the same description column.
/// Extracted (and unit-tested below) because the fit rule is the whole
/// point: at the default width several descriptions are LONGER than the
/// column ("Ctrl+J"'s is ~197 chars) and the old single-row
/// `truncate_ellipsis` silently ate their tails — a help screen whose
/// help is unreadable (maintainer complaint class, 2026-07-22).
fn help_rows(lines: &[&(&str, &str)], desc_w: i32) -> Vec<HelpRow> {
    let mut rows = Vec::new();
    for (key, desc) in lines {
        if key.is_empty() && desc.is_empty() {
            rows.push(HelpRow {
                key: String::new(),
                desc: String::new(),
            });
            continue;
        }
        let mut slices = text::wrap(desc, desc_w.max(16));
        if slices.is_empty() {
            slices.push(String::new());
        }
        for (i, slice) in slices.into_iter().enumerate() {
            rows.push(HelpRow {
                key: if i == 0 {
                    key.to_string()
                } else {
                    String::new()
                },
                desc: slice,
            });
        }
    }
    rows
}

pub fn open_help(cx: Scope, ctx: &UiCtx) {
    let ctx2 = ctx.clone();
    // Wide enough for comfortable one-row descriptions in the common
    // case (title-and-version header replaced the engine's AbstractTUI
    // logo — this is AbstractCode's help; maintainer finding
    // 2026-07-22). Long descriptions WRAP (help_rows) instead of
    // truncating, so the height request budgets a few continuation rows.
    let size = modal_size(
        96,
        (HELP_LINES.len() + crate::commands::HELP_EXTRA.len()) as i32 + 16,
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
        // Title: app identity + version. The version has ONE source —
        // Cargo.toml via env!("CARGO_PKG_VERSION") (crate::cli::VERSION);
        // CI/CD tags read the same manifest, so they cannot drift.
        col = col.child(title_row(
            &t,
            format!(
                "▲ AbstractCode v{} — rendered by AbstractTUI",
                crate::cli::VERSION
            ),
        ));
        let mut body = Element::new().style(LayoutStyle::column());
        let all_lines: Vec<&(&str, &str)> = HELP_LINES
            .iter()
            .chain(std::iter::once(&("", "")))
            .chain(crate::commands::HELP_EXTRA.iter())
            .collect();
        // Key gutter sized to the LONGEST key (POLISH-1/UX-17): the old
        // fixed 18 let 20-char keys ("/task <name> <title>") overprint
        // their own description. Clamped so one pathological key cannot
        // starve every description column.
        let key_w = all_lines
            .iter()
            .map(|(k, _)| text::width(k))
            .max()
            .unwrap_or(16)
            .clamp(8, 24);
        let gutter = key_w + 2;
        // −1 on the description column: keep it clear of the Scroll's
        // scrollbar glyph (UX-17: "(persiste┃" clipped UNDER the gutter
        // glyph in the captured frame). Wrap width == draw width, so
        // the draw's defensive clip below never actually fires.
        let content_w = size.w - 4;
        let desc_w = (content_w - gutter - 1).max(16);
        let rows = help_rows(&all_lines, desc_w);
        let n_rows = rows.len() as i32;
        for row in rows {
            let key = row.key;
            let desc = row.desc;
            let (accent, muted) = (t.accent, t.text_muted);
            body = body.child(
                Element::new()
                    .style(LayoutStyle::line(1))
                    .draw(move |canvas, rect| {
                        if !key.is_empty() {
                            let fitted_key = text::truncate_ellipsis(&key, key_w);
                            canvas.print(
                                Point::new(rect.x, rect.y),
                                &fitted_key,
                                accent,
                                Rgba::TRANSPARENT,
                            );
                        }
                        let avail = (rect.right() - rect.x - gutter - 1).max(0);
                        let fitted = text::truncate_ellipsis(&desc, avail);
                        canvas.print(
                            Point::new(rect.x + gutter, rect.y),
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
                .content_size(content_w, n_rows)
                .layout(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)))
                .element(mcx, &t)
                .autofocus()
                .build(),
        );
        col = col.child(hint_row(&t, "↑↓ scroll · Esc closes".into()));
        col.build()
    });
}

#[cfg(test)]
mod tests {
    use super::normalize_allowed_path;
    use std::path::Path;

    #[test]
    fn normalize_strips_trailing_slashes_so_dupes_collapse() {
        let home = Path::new("/home/me");
        assert_eq!(
            normalize_allowed_path("/srv/data/", home).unwrap(),
            "/srv/data"
        );
        assert_eq!(
            normalize_allowed_path("/srv/data///", home).unwrap(),
            "/srv/data"
        );
        // A trailing-slash form normalizes to the SAME string as the bare
        // form — the modal's `allowed.contains(&p)` dedup then works.
        assert_eq!(
            normalize_allowed_path("/srv/data/", home).unwrap(),
            normalize_allowed_path("/srv/data", home).unwrap()
        );
        // Root is preserved (never emptied).
        assert_eq!(normalize_allowed_path("/", home).unwrap(), "/");
        assert_eq!(normalize_allowed_path("//", home).unwrap(), "/");
    }

    #[test]
    fn normalize_expands_leading_tilde_against_home() {
        let home = Path::new("/home/me");
        assert_eq!(normalize_allowed_path("~", home).unwrap(), "/home/me");
        assert_eq!(
            normalize_allowed_path("~/proj/", home).unwrap(),
            "/home/me/proj"
        );
        // Another user's home cannot be resolved — refuse, never send a
        // literal ~user the gateway won't expand.
        assert!(normalize_allowed_path("~alice/x", home).is_err());
    }

    #[test]
    fn normalize_refuses_relative_paths_honestly() {
        let home = Path::new("/home/me");
        for rel in ["data", "./data", "../up", "a/b/c"] {
            let err = normalize_allowed_path(rel, home).unwrap_err();
            assert!(
                err.contains("not absolute"),
                "relative {rel:?} refused with a reason: {err}"
            );
        }
        assert!(normalize_allowed_path("   ", home).is_err());
    }

    #[test]
    fn help_rows_wrap_every_description_completely() {
        // The help modal fit rule (modal polish, 2026-07-23): at the
        // default width several HELP descriptions are LONGER than the
        // description column — they must WRAP into continuation rows
        // (key on the first row only), never truncate. Run against the
        // REAL help content at the real default geometry so a future
        // longer line cannot silently regress the fit.
        let all: Vec<&(&str, &str)> = crate::commands::HELP_LINES
            .iter()
            .chain(std::iter::once(&("", "")))
            .chain(crate::commands::HELP_EXTRA.iter())
            .collect();
        let key_w = all
            .iter()
            .map(|(k, _)| abstracttui::text::width(k))
            .max()
            .unwrap()
            .clamp(8, 24);
        // Default modal request is 96 wide → content 92 → desc column.
        let desc_w = 92 - (key_w + 2) - 1;
        let rows = super::help_rows(&all, desc_w);
        // Every entry's full text survives: reassemble each entry's
        // slices and compare against the source (whitespace-normalized —
        // wrapping only ever splits at whitespace).
        let mut idx = 0usize;
        for (key, desc) in &all {
            if key.is_empty() && desc.is_empty() {
                assert!(rows[idx].key.is_empty() && rows[idx].desc.is_empty());
                idx += 1;
                continue;
            }
            assert_eq!(rows[idx].key, *key, "key rides the first slice");
            let mut rebuilt = String::new();
            while idx < rows.len() && (rebuilt.is_empty() || rows[idx].key.is_empty()) {
                if !rebuilt.is_empty() && rows[idx].desc.is_empty() {
                    break; // the blank separator row
                }
                if !rebuilt.is_empty() {
                    rebuilt.push(' ');
                }
                rebuilt.push_str(&rows[idx].desc);
                idx += 1;
                // Stop before the next entry's first slice.
                if idx < rows.len() && !rows[idx].key.is_empty() {
                    break;
                }
                if idx < rows.len() && rows[idx].key.is_empty() && rows[idx].desc.is_empty() {
                    break;
                }
            }
            let normalize = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
            assert_eq!(
                normalize(&rebuilt),
                normalize(desc),
                "description for {key:?} survives wrapping whole"
            );
            // And every slice actually FITS the column.
        }
        for row in &rows {
            assert!(
                abstracttui::text::width(&row.desc) <= desc_w,
                "slice fits the description column: {:?}",
                row.desc
            );
        }
    }
}
