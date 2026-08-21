//! Attachment lane: `/attach`, the drop hook, chips, manager + picker.
//!
//! Model (attachments design, untracked/reviews/attachments-design.md):
//! files stage as PENDING chips (validated at attach — exists, regular,
//! ≤ cap when known) and upload at SEND on the worker thread — the only
//! shape that survives `/new` session rotation and makes removing a
//! chip a true no-op (session uploads are permanent server-side).
//! Custody stays with the UI until the run starts: upload failure
//! blocks the send and keeps the chips (the assistant's optimistic-
//! clear defect, fixed by construction here).
//!
//! Drop-as-paste rides the ENGINE seam (abstracttui 0.2.19, backlog
//! first-app/0273): `TextArea::on_paste` receives the raw paste,
//! `input::paste::classify` answers the pure spelling half (the
//! cross-terminal drop corpus), and this module answers the app half —
//! existence + kinds against the real filesystem. A verified drop
//! attaches directly (`PasteAction::Consume`) with a notice naming
//! Ctrl+O as the undo (chips out, raw text back): with the classifier's
//! strict quoting/escaping signals plus the existence gate, a real drop
//! vastly outweighs the pasted-path-as-prose case, and undo keeps that
//! case one keypress from intent. Attachments ride ONLY explicit
//! plain-prompt sends — never steers, queue drains, `/goal`, or entity
//! lanes.

use abstracttui::prelude::*;
use abstracttui::text;
use abstracttui::widgets::PasteAction;

use crate::paths::{expand_path_spelling, human_size, kind_caveat, resolve_drop};
use crate::store::{PendingAttachment, Store};
use crate::ui::UiCtx;

/// Soft ceiling: the runtime's active-attachments system message renders
/// at most 12 entries — beyond that the model sees a truncated list.
const ACTIVE_LIST_CAP: usize = 12;
/// Attach-time refusal ceiling when the gateway declared NO cap
/// (policy fetch failed / older gateway): the send-time upload buffers
/// the whole file plus one multipart copy on the worker, so "let the
/// server 413" must not mean gigabytes of transient allocation first.
/// Generous by design and labeled "client safety ceiling" in the
/// notice — never presented as a server rule.
pub(crate) const CLIENT_SAFETY_CEILING_BYTES: u64 = 512 * 1024 * 1024;
/// Text-like bytes inline into the model's context (120 KB/item
/// server-side); warn when the staged total crosses this.
const TEXT_INLINE_WARN_BYTES: u64 = 200 * 1024;

/// `/attach` dispatch: bare = manage (picker when nothing pending),
/// `clear` = discard, `preview [n|path]` = look at the bytes, anything
/// else = a path candidate.
pub(crate) fn dispatch_attach(cx: Scope, store: Store, ctx: &UiCtx, arg: Option<String>) {
    // PREVIEW runs BEFORE the lane guard: it reads a local file and
    // stages nothing, so the guard's reason ("attachments ride agent
    // runs only") is not true of it. Looking at a file is never a
    // lane-specific request shape.
    //
    // `preview` is a KEYWORD, like `clear` — a file literally named
    // "preview" still previews as `./preview`.
    match arg.as_deref().map(str::trim) {
        Some("preview") => return preview_dispatch(cx, store, ctx, ""),
        Some(rest) if rest.starts_with("preview ") => {
            return preview_dispatch(cx, store, ctx, rest["preview ".len()..].trim())
        }
        _ => {}
    }
    if entity_lane_refused(store) {
        return;
    }
    match arg.as_deref().map(str::trim) {
        None | Some("") => {
            if store.pending_attachments.with_untracked(|p| p.is_empty()) {
                open_picker_modal(cx, store, ctx);
            } else {
                open_manager(cx, store, ctx);
            }
        }
        Some("clear") => {
            let n = store.pending_attachments.with_untracked(|p| p.len());
            if n == 0 {
                store.notify("no pending attachments");
            } else {
                store.pending_attachments.set(Vec::new());
                // The undo slot names chips that no longer exist — a
                // later Ctrl+O must not claim to undo them (P1-2 class).
                store.paste_undo.set(None);
                store.notify(format!("{n} pending attachment(s) discarded"));
            }
        }
        Some(raw) => {
            attach_path(store, raw);
        }
    }
}

/// The one lane guard: attachments ride agent runs only (v1) — entity
/// visit turns POST `{text}` with no attachment surface, and flow-brain
/// summons are a different request shape.
fn entity_lane_refused(store: Store) -> bool {
    if matches!(store.focus.get_untracked(), crate::convo::Focus::Agent) {
        false
    } else {
        store.notify("attachments ride agent runs only (v1) — /focus agent first");
        true
    }
}

/// `/attach preview [n|path]`: no argument previews the single staged
/// chip (or points at the manager when there are several); a 1-based
/// index previews that chip; anything else is a PATH — previewing a
/// file you have not attached yet is the point of the command.
fn preview_dispatch(cx: Scope, store: Store, ctx: &UiCtx, arg: &str) {
    let n = store.pending_attachments.with_untracked(|p| p.len());
    if arg.is_empty() {
        match n {
            0 => store.notify("nothing staged — /attach preview <path> looks at any file"),
            1 => crate::ui::preview::open_pending(cx, store, ctx, 0),
            _ => {
                store.notify(format!(
                    "{n} attachments staged — /attach preview <1-{n}>, or p in the manager"
                ));
                open_manager(cx, store, ctx);
            }
        }
        return;
    }
    // An index only when it NAMES a staged chip: `1` with nothing
    // staged is a path candidate (and fails as one, honestly), never a
    // silent no-op.
    // Digits ONLY: `+2` parses as 2 for `usize`, which would preview a
    // chip when the operator named a file called `+2`.
    let as_index = arg
        .chars()
        .all(|c| c.is_ascii_digit())
        .then(|| arg.parse::<usize>().ok())
        .flatten();
    if let Some(ix) = as_index {
        if ix >= 1 && ix <= n {
            crate::ui::preview::open_pending(cx, store, ctx, ix - 1);
            return;
        }
        if n > 0 {
            store.notify(format!("no attachment {ix} — {n} staged"));
            return;
        }
    }
    crate::ui::preview::open_path(cx, store, ctx, arg);
}

/// Validate + stage one path as a pending chip, announcing the result.
/// Returns the CANONICAL path staged (`None` = refused) — undo slots
/// must key on it, never on the input spelling (P1-1: chips store
/// canonical paths; on macOS `/tmp`→`/private/tmp` a spelling-keyed
/// retain never matches).
pub(crate) fn attach_path(store: Store, raw: &str) -> Option<String> {
    attach_path_inner(store, raw, true)
}

/// The one validation funnel. `announce=false` suppresses the per-file
/// success notice (the DROP path emits one consolidated notice instead
/// — an N-file drop must not fire N+1 toasts); refusals ALWAYS notify
/// with the exact reason — a silent refusal reads as a dead command.
fn attach_path_inner(store: Store, raw: &str, announce: bool) -> Option<String> {
    let expanded = expand_path_spelling(raw);
    // Canonicalize: resolves symlinks + relative-against-cwd (typed
    // args are explicit intent — relative allowed HERE, never in the
    // drop detector), and makes the dedup key stable.
    let canon = match std::fs::canonicalize(&expanded) {
        Ok(p) => p,
        Err(_) => {
            store.notify(format!("no such file: {expanded}"));
            return None;
        }
    };
    let meta = match std::fs::metadata(&canon) {
        Ok(m) => m,
        Err(_) => {
            store.notify(format!("no such file: {expanded}"));
            return None;
        }
    };
    if meta.is_dir() {
        store.notify(format!(
            "{expanded} is a directory — attach files individually"
        ));
        return None;
    }
    if !meta.is_file() {
        store.notify(format!("{expanded} is not a regular file"));
        return None;
    }
    let path = canon.display().to_string();
    let name = canon
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());
    let size = meta.len();
    // Size pre-check: the gateway's declared cap when known; else the
    // client SAFETY ceiling only (labeled as such — never presented as
    // a server rule). The ceiling exists because the send-time read
    // buffers the whole file (+1 multipart copy) on the worker: an
    // unbounded stage would trade one 413 for gigabytes of transient
    // allocation.
    let cap = store.max_attachment_bytes.get_untracked();
    if cap > 0 && size > cap {
        store.notify(format!(
            "{name} is {} — over the gateway's {} attachment limit",
            human_size(size),
            human_size(cap)
        ));
        return None;
    }
    if cap == 0 && size > CLIENT_SAFETY_CEILING_BYTES {
        store.notify(format!(
            "{name} is {} — over the client's {} safety ceiling (gateway cap unknown)",
            human_size(size),
            human_size(CLIENT_SAFETY_CEILING_BYTES)
        ));
        return None;
    }
    let dup = store
        .pending_attachments
        .with_untracked(|p| p.iter().any(|a| a.path == path));
    if dup {
        store.notify(format!("{name} is already attached"));
        return None;
    }
    store.pending_attachments.update(|p| {
        p.push(PendingAttachment {
            path: path.clone(),
            name: name.clone(),
            size,
            uploaded: None,
        })
    });
    if announce {
        match kind_caveat(&name) {
            Some(caveat) => store.notify(format!(
                "attached {name} ({}) — note: {caveat}",
                human_size(size)
            )),
            None => store.notify(format!(
                "attached {name} ({}) — rides your next message",
                human_size(size)
            )),
        }
    }
    post_stage_warnings(store);
    Some(path)
}

/// Once-per-change staging warnings: the >12 active-list truncation and
/// the text-inlining context cost.
fn post_stage_warnings(store: Store) {
    store.pending_attachments.with_untracked(|p| {
        if p.len() == ACTIVE_LIST_CAP + 1 {
            store.notify(format!(
                "only the first {ACTIVE_LIST_CAP} attachments render in the model's active list"
            ));
        }
        let text_total: u64 = p
            .iter()
            .filter(|a| kind_caveat(&a.name).is_none())
            .map(|a| a.size)
            .sum();
        if text_total > TEXT_INLINE_WARN_BYTES {
            // Warn exactly when the newest attach crossed the line
            // (total minus the newest text-like item was still under).
            let newest_text = p
                .iter()
                .rev()
                .find(|a| kind_caveat(&a.name).is_none())
                .map(|a| a.size)
                .unwrap_or(0);
            if text_total.saturating_sub(newest_text) <= TEXT_INLINE_WARN_BYTES {
                store.notify(
                    "large text attachments inline into the model's context — consider fewer or smaller files",
                );
            }
        }
    });
}

/// The engine paste hook body (`TextArea::on_paste`): classify the raw
/// paste (engine, pure) → resolve against the filesystem (app) →
/// attach + Consume, or Insert as today. Directories notice; any
/// ambiguity inserts — the classifier's asymmetry policy continues
/// here.
pub(crate) fn handle_paste(store: Store, raw: &str) -> PasteAction {
    // Offer only on the agent lane — entity lanes have no attachment
    // surface (v1), so a drop there stays composer text.
    if !matches!(store.focus.get_untracked(), crate::convo::Focus::Agent) {
        return PasteAction::Insert;
    }
    let Some(paths) = abstracttui::input::paste::classify(raw) else {
        return PasteAction::Insert;
    };
    let Some(resolved) = resolve_drop(&paths) else {
        return PasteAction::Insert;
    };
    if resolved.iter().any(|r| r.is_dir) {
        // A folder drop is unmistakable intent, but the upload route
        // takes one FILE (recursing would sweep ignored/hidden files) —
        // say so; the paste inserts so nothing is eaten.
        store.notify("that's a folder — drop files, not folders");
        return PasteAction::Insert;
    }
    // Quiet per-file attaches: ONE consolidated drop notice below (an
    // N-file drop must not fire N+1 toasts). Refusals still notify per
    // file with their exact reason. The undo slot keys on the CANONICAL
    // paths attach_path staged — the spelling the chip actually carries
    // (P1-1: /tmp→/private/tmp symlinks made spelling-keyed undo a
    // silent no-op that left the chip AND restored the text).
    let mut attached: Vec<String> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut caveats: Vec<&'static str> = Vec::new();
    for r in &resolved {
        if let Some(canon) = attach_path_inner(store, &r.path, false) {
            names.push(
                std::path::Path::new(&canon)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| canon.clone()),
            );
            if let Some(c) = kind_caveat(names.last().unwrap()) {
                if !caveats.contains(&c) {
                    caveats.push(c);
                }
            }
            attached.push(canon);
        }
    }
    if attached.is_empty() {
        // Every candidate refused (dup/over-cap) — the notices said
        // why; keep the text so the paste isn't silently eaten.
        return PasteAction::Insert;
    }
    store
        .paste_undo
        .set(Some((raw.to_string(), attached.clone())));
    let head = if attached.len() == 1 {
        format!(
            "attached {} — Ctrl+O undoes (puts the path text back)",
            names[0]
        )
    } else {
        format!(
            "attached {} dropped files — Ctrl+O undoes (puts the path text back)",
            attached.len()
        )
    };
    store.notify(if caveats.is_empty() {
        head
    } else {
        format!("{head} — note: {}", caveats.join("; "))
    });
    PasteAction::Consume
}

/// Ctrl+O: undo the newest consumed drop — chips out, RAW paste text
/// into the composer (the pasted-path-as-prose escape hatch). Quiet
/// no-op when nothing to undo (the chord is taught only by the drop
/// notice). Honesty gates (P1-2 class): the undo fires only while the
/// chips it names still EXIST as pending — after they rode a run (or
/// were removed) restoring the text would inject stale path prose and
/// the notice would lie about undoing anything. Agent-lane only: the
/// slot was armed there, and the composer under entity focus belongs
/// to a different conversation.
pub(crate) fn undo_drop(store: Store, composer: &abstracttui::widgets::TextAreaState) {
    if !matches!(store.focus.get_untracked(), crate::convo::Focus::Agent) {
        return;
    }
    let Some((raw, attached)) = store.paste_undo.get_untracked() else {
        return;
    };
    store.paste_undo.set(None);
    let mut removed = 0usize;
    store.pending_attachments.update(|p| {
        let before = p.len();
        p.retain(|a| !attached.contains(&a.path));
        removed = before - p.len();
    });
    if removed == 0 {
        // The named chips are gone (rode a run / removed) — belt for a
        // slot the writers should already have cleared.
        return;
    }
    // Append to the draft (never clobber typing that happened since);
    // separate from existing prose so the restore never glues onto it
    // ("summarize this/tmp/f.txt").
    let existing = composer.text();
    if existing.trim().is_empty() {
        composer.set_text(raw.trim_end());
    } else {
        let sep = if existing.ends_with(char::is_whitespace) {
            ""
        } else {
            " "
        };
        composer.set_text(format!("{existing}{sep}{}", raw.trim_end()));
    }
    store.notify("drop undone — path text restored");
}

/// Session boundaries DISCARD pending chips (cached refs are
/// session-bound anyway; silently carrying files into an unrelated
/// conversation is the surprising behavior). Called by
/// `reset_session_state`.
pub(crate) fn discard_on_session_boundary(store: Store) {
    let n = store.pending_attachments.with_untracked(|p| p.len());
    if n > 0 {
        store.pending_attachments.set(Vec::new());
        store.notify(format!(
            "{n} pending attachment(s) discarded (session boundary)"
        ));
    }
    store.paste_undo.set(None);
}

/// One-line lane notices for sends that DON'T carry chips.
pub(crate) fn note_kept_for_steer(store: Store) {
    if store.pending_attachments.with_untracked(|p| !p.is_empty()) {
        store.notify("attachments stay pending — they ride your next new task, not steers");
    }
}
pub(crate) fn note_kept_for_goal(store: Store) {
    if store.pending_attachments.with_untracked(|p| !p.is_empty()) {
        store.notify(
            "pending attachments do not ride goal runs (v1) — send them with a plain message first",
        );
    }
}

/// Manager `p`/Enter: preview the chip under the cursor. Empty list =
/// a notice, never a modal over nothing.
fn preview_selected(cx: Scope, store: Store, ctx: &UiCtx, ix: usize) {
    if store.pending_attachments.with_untracked(|p| p.is_empty()) {
        store.notify("nothing staged to preview");
        return;
    }
    crate::ui::preview::open_pending(cx, store, ctx, ix);
}

/// Longest filename the chips row spells out. An attachment is
/// recognized by the HEAD of its name, and one long name (a screenshot,
/// a dated report) must not own the row that shows every staged file.
const NAME_MAX_CHARS: usize = 20;

/// A chip's display name: at most [`NAME_MAX_CHARS`] characters, plus an
/// ellipsis when there was more. The full name is never lost — the
/// preview header and the `/attach` manager both spell it out (the
/// manager also carries the whole path).
///
/// Counts CHARACTERS, not cells: the cap is about how much of a name to
/// spell, while the row's own packing works in cells (`text::width`), so
/// a CJK name still lays out correctly at twice the width.
fn chip_name(name: &str) -> String {
    let mut chars = name.chars();
    let head: String = chars.by_ref().take(NAME_MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

/// Unstage one chip, keyed by its CANONICAL path. The path is the
/// chip's identity (it is already the dedup key), so this is safe
/// against a list that changed since the caller last looked — an index
/// captured by a click handler can name a different file by the time
/// the click lands. Returns whether anything was removed.
///
/// ONE authority: the manager's `x` and the chips row's `×` both come
/// here, so the paste-undo rule (an armed undo must never outlive the
/// chips it names — P1-2 class) cannot drift between them.
pub(crate) fn remove_pending(store: Store, path: &str) -> bool {
    let mut removed = false;
    store.pending_attachments.update(|p| {
        let before = p.len();
        p.retain(|a| a.path != path);
        removed = p.len() != before;
    });
    if removed {
        store.paste_undo.set(None);
    }
    removed
}

/// The chip row's clickable name — clicking it previews the file.
fn chip_view(gcx: Scope, label: String, on_click: impl Fn() + 'static) -> View {
    let access = label.clone();
    clickable_run(gcx, label, access, false, on_click)
}

/// A one-glyph action beside a chip (`×` removes it). Separate element
/// from the name so a click can never resolve to the wrong action, and
/// DESTRUCTIVE ink on hover so the target reads as what it does.
fn chip_action(gcx: Scope, glyph: String, access: String, on_click: impl Fn() + 'static) -> View {
    clickable_run(gcx, glyph, access, true, on_click)
}

/// One CLICKABLE run of text. Deliberately not an engine `Button`:
/// Button is focusable, and adding focus stops to the chrome would
/// rewrite the app's Tab order (composer ⇄ transcript) for what is
/// purely a mouse affordance. Everything else follows Button's contract
/// exactly — press, then release INSIDE the rect decides the click
/// (pointer capture keeps the release routed here), state is cleared
/// BEFORE the callback runs (the disposal-safety law: the callback
/// opens a modal), and hover shifts ink only.
fn clickable_run(
    gcx: Scope,
    label: String,
    access: String,
    destructive: bool,
    on_click: impl Fn() + 'static,
) -> View {
    let width = text::width(&label).max(1);
    let hovered = gcx.signal(false);
    let pressed = gcx.signal(false);
    Element::new()
        .style(
            LayoutStyle::default()
                .width(Dimension::Cells(width))
                .height(Dimension::Cells(1))
                .shrink(0.0),
        )
        .role(abstracttui::ui::Role::Button)
        .access_label(access)
        .hover_signal(hovered)
        .on(abstracttui::ui::Phase::Bubble, move |ectx, ev| {
            let abstracttui::ui::UiEvent::Mouse(m) = ev else {
                return;
            };
            match m.kind {
                abstracttui::ui::MouseKind::Down(abstracttui::ui::MouseButton::Left) => {
                    pressed.set(true);
                    ectx.stop_propagation();
                }
                abstracttui::ui::MouseKind::Up(abstracttui::ui::MouseButton::Left) => {
                    let inside = ectx.current_rect().contains(m.pos);
                    let clicks = pressed.get_untracked() && inside;
                    pressed.set(false);
                    ectx.stop_propagation();
                    if clicks {
                        on_click();
                    }
                }
                _ => {}
            }
        })
        .child(dyn_view(
            LayoutStyle::default()
                .width(Dimension::Percent(1.0))
                .height(Dimension::Cells(1)),
            move || {
                let t = abstracttui::app::current_theme().tokens;
                let hot = if destructive { t.warn } else { t.accent };
                let (ink, bold) = if pressed.get() {
                    (hot, true)
                } else if hovered.get() {
                    (hot, false)
                } else if destructive {
                    (t.text_faint, false)
                } else {
                    (t.text_muted, false)
                };
                let label = label.clone();
                Element::new()
                    .style(LayoutStyle::default().width(Dimension::Percent(1.0)))
                    .draw(move |canvas, rect| {
                        if rect.is_empty() {
                            return;
                        }
                        let mut style = abstracttui::render::Style::new()
                            .fg(ink)
                            .bg(Rgba::TRANSPARENT);
                        if bold {
                            style = style.attrs(abstracttui::render::Attrs::BOLD);
                        }
                        let fitted = text::truncate_ellipsis(&label, rect.w);
                        canvas.print_styled(Point::new(rect.x, rect.y), &fitted, &style);
                    })
                    .build()
            },
        ))
        .build()
}

/// A plain (non-interactive) run of text in the chips row.
fn chip_label(text_: String, ink: Rgba) -> View {
    let width = text::width(&text_).max(1);
    Element::new()
        .style(
            LayoutStyle::default()
                .width(Dimension::Cells(width))
                .height(Dimension::Cells(1))
                .shrink(0.0),
        )
        .draw(move |canvas, rect| {
            if rect.is_empty() {
                return;
            }
            let fitted = text::truncate_ellipsis(&text_, rect.w);
            canvas.print(Point::new(rect.x, rect.y), &fitted, ink, Rgba::TRANSPARENT);
        })
        .build()
}

/// Chips row between the transcript spacer and the activity strip:
/// exists only while pending is non-empty (no reserved blank line).
///
/// Each file NAME is the affordance: clicking it opens the preview
/// (`ui::preview`). One element per chip, so the LAYOUT owns the hit
/// rectangles — no hand-rolled column arithmetic that can drift from
/// what was actually drawn.
///
/// `cx` is the ROOT scope, not the row's: it owns the modal a click
/// opens, which must outlive this row's next rebuild.
///
/// NOTE: chrome-height estimates (CHROME_ROWS) deliberately exclude
/// this sometimes-present row — those are scroll ESTIMATES only, and
/// one row of slack while chips show is benign.
pub(crate) fn chips_row(cx: Scope, store: Store, ctx: &UiCtx) -> View {
    let ctx = ctx.clone();
    dyn_view_scoped(LayoutStyle::default().shrink(0.0), move |gcx| {
        let pending = store.pending_attachments.get();
        if pending.is_empty() {
            return Element::new().build();
        }
        let t = abstracttui::app::current_theme().tokens;
        // Track the viewport so the row re-packs on resize.
        let width = abstracttui::app::use_viewport(gcx).get().w;
        const MARKER: &str = "📎 ";
        const SEP: &str = " · ";
        // The remove target, and the gap that separates it from the
        // name so a click cannot land on the wrong one.
        const REMOVE: &str = "×";
        const REMOVE_GAP: &str = " ";
        let per_chip_extra = text::width(REMOVE_GAP) + text::width(REMOVE);
        // One column of inset on each side (the chrome convention) plus
        // the marker.
        let mut budget = width - 2 - text::width(MARKER);
        let mut row = Element::new()
            .style(
                LayoutStyle::row()
                    .width(Dimension::Percent(1.0))
                    .height(Dimension::Cells(1))
                    .shrink(0.0),
            )
            .child(chip_label(" ".into(), t.text_faint))
            .child(chip_label(MARKER.into(), t.text_faint));
        // The narrowest label worth rendering: below this a chip is all
        // ellipsis and nothing is gained by keeping it.
        const MIN_LABEL: i32 = 8;
        let mut shown = 0usize;
        for (index, a) in pending.iter().enumerate() {
            let full = format!("{} ({})", chip_name(&a.name), human_size(a.size));
            let sep_w = if shown == 0 { 0 } else { text::width(SEP) };
            // Room for the "+N more" tail whenever anything is left
            // over — a chip cut off at the screen edge would hide a
            // staged file that IS going to ride the next send. Sized
            // from the real tail text, not a guessed constant: "+10
            // more" is wider than "+9 more".
            let tail = if index + 1 < pending.len() {
                text::width(&format!("{SEP}+{} more", pending.len() - shown))
            } else {
                0
            };
            let room = budget - tail - sep_w - per_chip_extra;
            // A name too long for the row is TRUNCATED, never dropped —
            // dropping the only chip left the row reading "📎 · +1 more"
            // with a dangling separator and no name at all (adversary
            // finding P2, 2026-08-21). Only the first chip earns this:
            // once the row is full the rest belong in the tail, where
            // they are counted.
            let label = if text::width(&full) <= room {
                full
            } else if shown == 0 && room >= MIN_LABEL {
                text::truncate_ellipsis(&full, room)
            } else {
                break;
            };
            let cost = sep_w + text::width(&label) + per_chip_extra;
            budget -= cost;
            if shown > 0 {
                row = row.child(chip_label(SEP.into(), t.text_faint));
            }
            // Both actions key on the chip's PATH, never on this
            // index: the row is rebuilt from the signal, but a click
            // resolves against whatever is staged when it LANDS.
            let path = a.path.clone();
            let name = a.name.clone();
            let ctx = ctx.clone();
            let preview_path = path.clone();
            row = row.child(chip_view(gcx, label, move || {
                crate::ui::preview::open_chip(cx, store, &ctx, &preview_path)
            }));
            row = row.child(chip_label(REMOVE_GAP.into(), t.text_faint));
            row = row.child(chip_action(
                gcx,
                REMOVE.into(),
                format!("remove {name}"),
                move || {
                    remove_pending(store, &path);
                },
            ));
            shown += 1;
        }
        if shown < pending.len() {
            // No leading separator when nothing precedes it — a row
            // reading " · +1 more" separates the tail from nothing.
            let lead = if shown == 0 { "" } else { SEP };
            row = row.child(chip_label(
                format!("{lead}+{} more", pending.len() - shown),
                t.text_faint,
            ));
        }
        row.build()
    })
}

/// Pending manager (queue-modal pattern): ↑↓ select, `x` remove,
/// `c` clear, `b` browse (picker), Esc/Enter close.
pub(crate) fn open_manager(cx: Scope, store: Store, ctx: &UiCtx) {
    let ctx2 = ctx.clone();
    let vp = abstracttui::app::current_viewport();
    let size = Size::new(84.min(vp.w - 4).max(30), 14.min(vp.h - 6).max(8));
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let cursor = mcx.signal(0usize);
        let move_cursor = move |delta: i64| {
            let n = store.pending_attachments.with_untracked(|p| p.len());
            if n == 0 {
                return;
            }
            cursor.update(|c| *c = (*c as i64 + delta).clamp(0, n as i64 - 1) as usize);
        };
        let remove_selected = move || {
            // Through the ONE authority (`remove_pending`, path-keyed):
            // it also clears the drop-undo slot, because any removal may
            // orphan it (P1-2 class: an armed undo must never outlive
            // the chips it names).
            let ix = cursor.get_untracked();
            let path = store
                .pending_attachments
                .with_untracked(|p| p.get(ix).map(|a| a.path.clone()));
            if let Some(path) = path {
                remove_pending(store, &path);
            }
            let n = store.pending_attachments.with_untracked(|p| p.len());
            cursor.update(|c| *c = (*c).min(n.saturating_sub(1)));
        };
        Element::new()
            .style(LayoutStyle::column().gap(1).padding(Edges::all(1)))
            .focusable()
            .autofocus()
            .shortcut(KeyChord::plain(Key::Escape), {
                let ctx = ctx2.clone();
                move |_| ctx.close_modal()
            })
            // Enter OPENS the selected chip (the file-manager idiom);
            // Esc is this app's universal close, so nothing is lost.
            .shortcut(KeyChord::plain(Key::Enter), {
                let ctx = ctx2.clone();
                move |_| preview_selected(cx, store, &ctx, cursor.get_untracked())
            })
            .shortcut(KeyChord::plain(Key::Char('p')), {
                let ctx = ctx2.clone();
                move |_| preview_selected(cx, store, &ctx, cursor.get_untracked())
            })
            .shortcut(KeyChord::plain(Key::Up), move |_| move_cursor(-1))
            .shortcut(KeyChord::plain(Key::Down), move |_| move_cursor(1))
            .shortcut(KeyChord::plain(Key::Char('x')), move |_| remove_selected())
            .shortcut(KeyChord::plain(Key::Char('c')), move |_| {
                let n = store.pending_attachments.with_untracked(|p| p.len());
                if n > 0 {
                    store.pending_attachments.set(Vec::new());
                    store.paste_undo.set(None);
                    store.notify(format!("{n} pending attachment(s) discarded"));
                }
                cursor.set(0);
            })
            .shortcut(KeyChord::plain(Key::Char('b')), {
                let ctx = ctx2.clone();
                move |_| {
                    // Swap to the picker (atomic replace — open_modal
                    // retires this modal in the same flush).
                    open_picker_modal(cx, store, &ctx);
                }
            })
            .child(dyn_view(LayoutStyle::line(1), move || {
                let t2 = abstracttui::app::current_theme().tokens;
                let n = store.pending_attachments.with(|p| p.len());
                let title =
                    format!("pending attachments — {n} staged · upload at send · session uploads are permanent");
                Element::new()
                    .style(LayoutStyle::line(1).shrink(0.0))
                    .draw(move |canvas, rect| {
                        let fitted = text::truncate_ellipsis(&title, (rect.w - 1).max(4));
                        canvas.print(Point::new(rect.x, rect.y), &fitted, t2.accent, Rgba::TRANSPARENT);
                    })
                    .build()
            }))
            .child(dyn_view(
                LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),
                move || {
                    let t2 = abstracttui::app::current_theme().tokens;
                    let items = store.pending_attachments.get();
                    let cur = cursor.get().min(items.len().saturating_sub(1));
                    let rows: Vec<String> = items
                        .iter()
                        .enumerate()
                        .map(|(i, a)| {
                            format!("{}. {} ({}) — {}", i + 1, a.name, human_size(a.size), a.path)
                        })
                        .collect();
                    Element::new()
                        .style(LayoutStyle::column().grow(1.0).basis(Dimension::Cells(0)))
                        .draw(move |canvas, rect| {
                            if rows.is_empty() {
                                canvas.print(
                                    Point::new(rect.x + 1, rect.y),
                                    "nothing staged — /attach <path> adds a file, b browses, p previews",
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
                                    canvas.fill(Rect::new(rect.x, y, rect.w, 1), ' ', ink, bg);
                                }
                                let fitted = text::truncate_ellipsis(row, (rect.w - 2).max(4));
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
                        let hint =
                            "↑↓ select · Enter/p preview · x remove · c clear · b browse · Esc closes";
                        let fitted = text::truncate_ellipsis(hint, (rect.w - 1).max(4));
                        canvas.print(Point::new(rect.x, rect.y), &fitted, faint, Rgba::TRANSPARENT);
                    })
                    .build()
            })
            .build()
    });
}

/// The navigating picker (engine `FilePicker`, 0.2.19): browse from the
/// workspace root (fallback cwd), multi-select with Space, Enter
/// commits → every pick goes through the SAME `attach_path` validation
/// as typed paths.
pub(crate) fn open_picker_modal(cx: Scope, store: Store, ctx: &UiCtx) {
    let start = ctx
        .workspace_root
        .clone()
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.display().to_string())
        })
        .unwrap_or_else(|| "/".to_string());
    let ctx2 = ctx.clone();
    let vp = abstracttui::app::current_viewport();
    let size = Size::new(90.min(vp.w - 4).max(40), 22.min(vp.h - 4).max(10));
    ctx.open_modal(cx, size, move |mcx| {
        let ctx3 = ctx2.clone();
        Element::new()
            .style(LayoutStyle::column().padding(Edges::all(1)))
            .shortcut(KeyChord::plain(Key::Escape), {
                let ctx = ctx2.clone();
                move |_| ctx.close_modal()
            })
            .child(
                abstracttui::widgets::FilePicker::new(abstracttui::widgets::StdFileSource::new())
                    .start_in(&start)
                    .multi_select(true)
                    .show_sizes(true)
                    .layout(LayoutStyle::column().grow(1.0).basis(Dimension::Cells(0)))
                    .on_pick(move |paths| {
                        for p in &paths {
                            // attach_path notices per file (success +
                            // refusal reasons); close even when
                            // everything refused — reopening is one
                            // /attach away.
                            let _ = attach_path(store, p);
                        }
                        ctx3.close_modal();
                    })
                    .view(mcx),
            )
            .build()
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chip_names_are_capped_at_twenty_characters() {
        // At the cap and under, the name is spelled in full.
        assert_eq!(chip_name("notes.md"), "notes.md");
        let exactly = "a".repeat(NAME_MAX_CHARS);
        assert_eq!(chip_name(&exactly), exactly);
        // One over: the first 20 characters, then the ellipsis.
        let over = format!("{exactly}b");
        assert_eq!(chip_name(&over), format!("{exactly}…"));
        // The real case that prompted the cap.
        assert_eq!(
            chip_name("Screenshot 2026-08-21 at 4.37.29 AM.png"),
            "Screenshot 2026-08-2…"
        );
        // Characters, not bytes: a multibyte name keeps 20 of them.
        let cjk = "文".repeat(25);
        let capped = chip_name(&cjk);
        assert_eq!(capped.chars().count(), NAME_MAX_CHARS + 1);
        assert!(capped.ends_with('…'));
    }
}
