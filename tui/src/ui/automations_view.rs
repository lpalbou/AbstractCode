//! `/automations` and `/schedule`: the gateway's automations in the terminal.
//!
//! Same behaviour and contract as the Observer, the Assistant and the
//! ui-kit panel (`crate::automations` holds the shared rules):
//! - the list: state as text + icon ("Active ▶"), cadence, what runs NOW
//!   (only `current_occurrence`), the NEXT run (only `next_fire_at`),
//!   attention; archived rows hidden until asked (`h`);
//! - one automation: its header (the folder is `workspace_root`, browsed
//!   through the gateway's workspace routes with `w`), the waits that need
//!   you (tool approvals `y`/`n`, questions and events with Enter), its
//!   runs as chat pairs, and every control (pause/resume, run now, stop
//!   current, revise, archive — which hides and stops, never deletes);
//! - Discuss (`d`) forks the automation at the selected run and SWITCHES
//!   this terminal to the new chat, in place (one session pool);
//! - `/schedule` creates one from the current workflow in four steps
//!   (task, when, context, tools) with the same body as every client.
//!
//! Nothing here executes anything: every action is a gateway route, sent
//! through `Cmd::Automations` (one thread per action) and read back.

use std::rc::Rc;

use abstracttui::prelude::*;
use abstracttui::widgets::TextInput;

use crate::automations::{self as auto, Control, Summary};
use crate::gateway::automations::AutoCmd;
use crate::runner::Cmd;
use crate::store::Store;
use crate::transcript::Item;
use crate::ui::cards::{draw_cards, hint_bar, note_lines, Card, CardLine, Ink};
use crate::ui::modals::{
    draw_rows, hint_row, modal_size, open_picker, title_row, wrap_lines, Mark, Picker, RowSpec,
};
use crate::ui::UiCtx;

/// Re-read cadence while an automations screen is open (nothing polls once
/// it closes). The gateway has no change cursor: full pages are re-read.
const POLL: std::time::Duration = std::time::Duration::from_secs(15);

fn send(ctx: &UiCtx, cmd: AutoCmd) {
    // A dead runner loop already posted its own notice (runner panic
    // surfacing); nothing more to say here.
    let _ = ctx.send(Cmd::Automations(cmd));
}

fn open_id(store: Store) -> Option<String> {
    store
        .automations
        .with_untracked(|v| v.detail.as_ref().map(|d| d.id.clone()))
}

fn refresh(store: Store, ctx: &UiCtx) {
    store.automations.update(|v| v.loading = true);
    send(
        ctx,
        AutoCmd::Refresh {
            open: open_id(store),
        },
    );
}

/// Send one `automation.*` command for `s`, if the control applies now.
/// The id is minted once per action and reused only for a transport retry.
pub(crate) fn command(store: Store, ctx: &UiCtx, s: &Summary, control: Control) {
    let busy = store.automations.with_untracked(|v| v.busy);
    if let Err(why) = auto::control_state(s, control, busy) {
        store
            .automations
            .update(|v| v.error = format!("{}: {why}", control.button()));
        return;
    }
    let Some(command_type) = control.command_type() else {
        return;
    };
    let key = format!("{}:{command_type}", s.id);
    let mut command_id = String::new();
    store.automations.update(|v| {
        command_id = v.ids.id_for(&key, crate::config::mint_session_id);
        v.busy = true;
        v.error.clear();
        v.notice = control.busy_notice().to_string();
    });
    send(
        ctx,
        AutoCmd::Command {
            id: s.id.clone(),
            command_id,
            command_type: command_type.to_string(),
        },
    );
}

/// Space on an automation: flip its Active switch (pause when active,
/// resume when paused), or say why it cannot change now.
pub(crate) fn switch_active(store: Store, ctx: &UiCtx, s: &Summary) {
    let busy = store.automations.with_untracked(|v| v.busy);
    match auto::active_switch(s, busy) {
        Ok(_) => command(store, ctx, s, auto::active_command(s)),
        Err(why) => store
            .automations
            .update(|v| v.error = format!("Active: {why}")),
    }
}

/// The Active switch row of one automation (`[x] Active — …`), the
/// automation screen's first header row.
pub(crate) fn active_row(s: &Summary, busy: bool) -> RowSpec {
    RowSpec {
        text: format!(
            "{} — {}",
            auto::active_label(),
            auto::active_detail(s, busy)
        ),
        header: false,
        checked: Some(active_mark(s, busy)),
        dim: false,
    }
}

/// The switch's mark: on/off, or unavailable (with the state kept while a
/// command is in flight).
fn active_mark(s: &Summary, busy: bool) -> Mark {
    match auto::active_switch(s, busy) {
        Ok(on) => Mark::switch(on),
        // In flight: keep showing the current state, not "unavailable".
        Err(_) if busy && !s.legacy && (s.status == "active" || s.status == "paused") => {
            Mark::switch(s.status == "active")
        }
        Err(_) => Mark::Unavailable,
    }
}

/// `[x] Active` / `[ ] Active` / `[-] Active` — the card's switch.
pub(crate) fn active_switch_text(s: &Summary, busy: bool) -> String {
    format!("{}{}", active_mark(s, busy).marker(), auto::active_label())
}

/// The status lines under a title: notice, error, availability.
fn status_lines(v: &auto::View) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(Err(why)) = &v.availability {
        out.push(why.clone());
    }
    if !v.error.is_empty() {
        out.push(v.error.clone());
    }
    if !v.notice.is_empty() {
        out.push(v.notice.clone());
    }
    out
}

fn arm_poll(mcx: Scope, store: Store, ctx: UiCtx) {
    // Owned by the modal scope: disposal cancels it, so nothing ticks once
    // the screen closes (the engine's zero-wakeup idle rule).
    let _ = abstracttui::reactive::interval(mcx, POLL, move || refresh(store, &ctx));
}

// ---------------------------------------------------------------------------
// The list
// ---------------------------------------------------------------------------

/// What the cursor is on in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListTarget {
    Automation(String),
    /// The `Archived · N` line (Enter opens/closes it).
    ArchivedLine,
    /// An archived automation (Enter or `u` unarchives it).
    Archived(String),
}

/// The archive question, verbatim from the Code web header.
pub fn archive_question(title: &str) -> String {
    format!("Archive “{title}”? It will not run again; its history stays readable.")
}

/// The list as cards (pure, test-pinned): each automation is its name (+
/// the "waiting for you" badge), `↻ every 24 h · last 3 h ago`, then
/// `next in 20 h` with the Active switch at the right; then the quiet
/// `Archived · N` line and, when open, the archived ones with Unarchive.
/// `confirm` = the automation whose Archive question is showing.
pub fn list_cards(v: &auto::View, now: i64, confirm: Option<&str>) -> (Vec<Card>, Vec<ListTarget>) {
    let mut cards = Vec::new();
    let mut targets = Vec::new();
    match &v.list {
        None => cards.push(Card::note("Loading automations…")),
        Some(Err(e)) => cards.push(Card::fixed(vec![CardLine::new(
            format!("The automations could not be read: {e}"),
            Ink::Error,
        )])),
        Some(Ok(items)) => {
            let shown = auto::visible(items);
            for s in &shown {
                let (line1, line2) = auto::card_lines(s, now);
                let mut lines = vec![CardLine::new(s.title.clone(), Ink::Title)
                    .right(auto::waiting_badge(s).unwrap_or(""))];
                lines.push(CardLine::new(line1, Ink::Faint).indent(2));
                let switch_ink = match active_mark(s, v.busy) {
                    Mark::On => Ink::On,
                    Mark::Unavailable => Ink::Faint,
                    _ => Ink::Text,
                };
                lines.push(
                    CardLine::new(line2, Ink::Faint)
                        .indent(2)
                        .right(active_switch_text(s, v.busy))
                        .right_ink(switch_ink),
                );
                if confirm == Some(s.id.as_str()) {
                    lines.push(CardLine::new(archive_question(&s.title), Ink::Accent).indent(2));
                    lines.push(CardLine::new("y Archive · n Keep it", Ink::Accent).indent(2));
                }
                cards.push(Card::new(lines));
                targets.push(ListTarget::Automation(s.id.clone()));
            }
            if shown.is_empty() {
                cards.push(Card::note(
                    "No automations yet. n creates one from the current workflow (/schedule).",
                ));
            }
        }
    }
    if let Some(line) = auto::archived_line(v.archived_count) {
        cards.push(Card::new(vec![CardLine::new(line, Ink::Faint)]));
        targets.push(ListTarget::ArchivedLine);
        if v.archived_open {
            match &v.archived {
                None => cards.push(Card::note("Loading archived automations…")),
                Some(Err(e)) => cards.push(Card::fixed(vec![CardLine::new(
                    format!("Archived automations unavailable: {e}"),
                    Ink::Error,
                )
                .indent(2)])),
                Some(Ok(items)) => {
                    for s in items {
                        cards.push(Card::new(vec![CardLine::new(s.title.clone(), Ink::Faint)
                            .indent(2)
                            .right(Control::Unarchive.button())]));
                        targets.push(ListTarget::Archived(s.id.clone()));
                    }
                }
            }
        }
    }
    (cards, targets)
}

/// The list screen's key hints.
pub const LIST_HINTS: &[(&str, &str)] = &[
    ("↑↓", ""),
    ("Enter", "opens"),
    ("space", "Active"),
    ("g", "Run now"),
    ("x", "Stop"),
    ("e", "Edit"),
    ("a", "Archive"),
    ("u", "Unarchive"),
    ("n", "New"),
    ("r", "Refresh"),
    ("Esc", "closes"),
];

/// `/automations` — every automation of the signed-in gateway user.
pub fn open_automations(cx: Scope, store: Store, ctx: &UiCtx) {
    store.automations.update(|v| {
        v.detail = None;
        v.loading = true;
        v.notice.clear();
    });
    send(ctx, AutoCmd::Refresh { open: None });
    let ctx2 = ctx.clone();
    let size = modal_size(160, 40);
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let cursor = mcx.signal(0usize);
        let confirm_archive = mcx.signal(Option::<String>::None);
        arm_poll(mcx, store, ctx2.clone());
        let targets = move || {
            let now = auto::now_unix();
            store
                .automations
                .with_untracked(|v| list_cards(v, now, None).1)
        };
        let target = move || targets().get(cursor.get_untracked()).cloned();
        let summary_of = move |id: &str| -> Option<Summary> {
            store.automations.with_untracked(|v| {
                let live = v.list.as_ref().and_then(|l| l.as_ref().ok());
                let arch = v.archived.as_ref().and_then(|l| l.as_ref().ok());
                live.into_iter()
                    .chain(arch)
                    .flatten()
                    .find(|s| s.id == id)
                    .cloned()
            })
        };
        let selected = move || match target() {
            Some(ListTarget::Automation(id)) => summary_of(&id),
            _ => None,
        };
        let move_cursor = move |delta: i64| {
            confirm_archive.set(None);
            let n = targets().len();
            if n > 0 {
                cursor.update(|c| *c = (*c as i64 + delta).clamp(0, n as i64 - 1) as usize);
            }
        };
        let act = {
            let ctx = ctx2.clone();
            move |control: Control| {
                confirm_archive.set(None);
                if let Some(s) = selected() {
                    command(store, &ctx, &s, control);
                }
            }
        };
        let switch = {
            let ctx = ctx2.clone();
            move || {
                confirm_archive.set(None);
                if let Some(s) = selected() {
                    switch_active(store, &ctx, &s);
                }
            }
        };
        let archive = move || {
            let Some(s) = selected() else { return };
            if let Err(why) = auto::control_state(&s, Control::Archive, false) {
                store
                    .automations
                    .update(|v| v.error = format!("Archive: {why}"));
            } else {
                store.automations.update(|v| v.error.clear());
                confirm_archive.set(Some(s.id.clone()));
            }
        };
        let confirm_yes = {
            let ctx = ctx2.clone();
            move || {
                let Some(id) = confirm_archive.get_untracked() else {
                    return false;
                };
                confirm_archive.set(None);
                if let Some(s) = summary_of(&id) {
                    command(store, &ctx, &s, Control::Archive);
                }
                true
            }
        };
        let unarchive = {
            let ctx = ctx2.clone();
            move || {
                confirm_archive.set(None);
                if let Some(ListTarget::Archived(id)) = target() {
                    if let Some(s) = summary_of(&id) {
                        command(store, &ctx, &s, Control::Unarchive);
                    }
                }
            }
        };
        let edit = {
            let ctx = ctx2.clone();
            move || {
                confirm_archive.set(None);
                if let Some(s) = selected() {
                    crate::ui::rail_view::open_automation_settings(cx, store, &ctx, &s.id);
                }
            }
        };
        let enter = {
            let ctx = ctx2.clone();
            let unarchive = unarchive.clone();
            move || match target() {
                Some(ListTarget::Automation(id)) => open_automation(cx, store, &ctx, &id),
                Some(ListTarget::ArchivedLine) => {
                    store
                        .automations
                        .update(|v| v.archived_open = !v.archived_open);
                }
                Some(ListTarget::Archived(_)) => unarchive(),
                None => {}
            }
        };
        let new = {
            let ctx = ctx2.clone();
            move || open_schedule(cx, store, &ctx, None)
        };
        let key = |c: char| KeyChord::plain(Key::Char(c));
        Element::new()
            .style(LayoutStyle::column().padding(Edges::all(1)))
            .focusable()
            .autofocus()
            .shortcut(KeyChord::plain(Key::Escape), {
                let ctx = ctx2.clone();
                move |_| {
                    if confirm_archive.get_untracked().is_some() {
                        confirm_archive.set(None);
                    } else {
                        ctx.close_modal()
                    }
                }
            })
            .shortcut(KeyChord::plain(Key::Up), move |_| move_cursor(-1))
            .shortcut(KeyChord::plain(Key::Down), move |_| move_cursor(1))
            .shortcut(KeyChord::plain(Key::Enter), move |_| enter())
            .shortcut(key('y'), move |_| {
                confirm_yes();
            })
            .shortcut(key('n'), move |_| {
                if confirm_archive.get_untracked().is_some() {
                    confirm_archive.set(None);
                } else {
                    new()
                }
            })
            .shortcut(KeyChord::plain(Key::Char(' ')), {
                let switch = switch.clone();
                move |_| switch()
            })
            // `p` (the old pause/resume key) switches Active too.
            .shortcut(key('p'), move |_| switch())
            .shortcut(key('g'), {
                let act = act.clone();
                move |_| act(Control::RunNow)
            })
            .shortcut(key('x'), {
                let act = act.clone();
                move |_| act(Control::StopCurrent)
            })
            .shortcut(key('e'), move |_| edit())
            .shortcut(key('a'), move |_| archive())
            .shortcut(key('u'), move |_| unarchive())
            .shortcut(key('r'), {
                let ctx = ctx2.clone();
                move |_| refresh(store, &ctx)
            })
            .child(title_row(&t, "Automations".into()))
            .child(dyn_view(LayoutStyle::column().shrink(0.0), move || {
                let t2 = abstracttui::app::current_theme().tokens;
                note_lines(&t2, &store.automations.with(status_lines), 8)
            }))
            .child(dyn_view(
                LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),
                move || {
                    let now = auto::now_unix();
                    let confirm = confirm_archive.get();
                    let (cards, targets) = store
                        .automations
                        .with(|v| list_cards(v, now, confirm.as_deref()));
                    let cur = cursor.get().min(targets.len().saturating_sub(1));
                    draw_cards(cards, cur)
                },
            ))
            .child(hint_bar(&t, LIST_HINTS, 8))
            .child(note_lines(&t, &[auto::run_now_key_line()], 8))
            .build()
    });
}

// ---------------------------------------------------------------------------
// One automation
// ---------------------------------------------------------------------------

/// A selectable target in the automation view.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Earlier,
    Wait(auto::Wait),
    Run(u64),
}

/// The header lines of one automation (pure, test-pinned): the Code web
/// header — `[x] Active · every 24 h · last 3 h ago · next in 14 h` (+ the
/// "waiting for you" badge), the workspace as a short name — then the
/// facts the terminal keeps (runs, revision, context, tools, workflow).
pub fn header_lines(s: &Summary, def: Option<&auto::Definition>, now: i64) -> Vec<String> {
    let mut first = auto::timing_line(s, now);
    if let Some(badge) = auto::waiting_badge(s) {
        first.push_str(&format!(" · {badge}"));
    }
    let mut out = vec![first];
    if let Some(cur) = auto::current_label(s) {
        out.push(cur);
    }
    if let Some(root) = &s.workspace_root {
        out.push(format!(
            "Workspace {} (w browses it)",
            crate::ui::rail_view::short_name(root)
        ));
    }
    let mut facts = vec![format!(
        "{} {}",
        s.occurrence_count,
        if s.occurrence_count == 1 {
            "run"
        } else {
            "runs"
        }
    )];
    if let Some(rev) = def.map(|d| d.revision).or(s.revision) {
        facts.push(format!("Revision {rev}"));
    }
    facts.push(auto::context_label(&s.context_mode).to_string());
    if let Some(d) = def {
        facts.push(if d.tool_approval == "ask" {
            "tools: ask me before each tool call".into()
        } else {
            format!("tools: {}", auto::TOOL_APPROVAL_CONSENT.to_lowercase())
        });
        if !d.workflow_id.is_empty() {
            facts.push(format!("workflow {}", d.workflow_id));
        }
        if s.context_mode == "growing" && !d.growing.is_empty() {
            facts.push(format!(
                "growing context {}",
                serde_json::Value::Object(d.growing.clone())
            ));
        }
    }
    out.push(facts.join(" · "));
    if s.attention.unseen_count > 0 || s.attention.pending_waits > 0 {
        out.push(format!("attention: {}", auto::attention_label(s)));
    }
    if s.status == "archived" {
        out.push(
            "Archived: it will not run again; its history stays readable (Discuss still works)."
                .into(),
        );
    }
    if s.legacy {
        out.push("Legacy schedule: managed with its existing controls (Observer run view).".into());
    }
    out
}

/// The automation screen's key hints (the header's buttons first).
pub const DETAIL_HINTS: &[(&str, &str)] = &[
    ("g", "Run now"),
    ("x", "Stop"),
    ("e", "Edit"),
    ("a", "Archive"),
    ("u", "Unarchive"),
    ("space", "Active"),
    ("↑↓", ""),
    ("y/n", "approve/deny"),
    ("Enter", "answers"),
    ("d", "Discuss run"),
    ("w", "folder"),
    ("r", "Refresh"),
    ("Esc", "back"),
];

/// The body rows and their targets: waits that need you, the unseen
/// attention items, then every loaded run as a chat pair, oldest first.
pub(crate) fn detail_rows(d: &auto::Detail, width: i32) -> (Vec<RowSpec>, Vec<(usize, Target)>) {
    let mut rows = Vec::new();
    let mut targets = Vec::new();
    let push = |rows: &mut Vec<RowSpec>, text: String, dim: bool| {
        rows.push(RowSpec {
            text,
            header: false,
            checked: None,
            dim,
        })
    };
    let wrap_into = |rows: &mut Vec<RowSpec>, prefix: &str, text: &str, dim: bool| {
        let pad = " ".repeat(prefix.chars().count());
        for (i, line) in wrap_lines(text, (width - prefix.chars().count() as i32).max(8), None)
            .into_iter()
            .enumerate()
        {
            let lead = if i == 0 { prefix } else { pad.as_str() };
            rows.push(RowSpec {
                text: format!("{lead}{line}"),
                header: false,
                checked: None,
                dim,
            });
        }
    };
    if let Some(s) = &d.summary {
        let waits = &s.attention.waits;
        if !waits.is_empty() || !s.attention.items.is_empty() {
            push(&mut rows, "needs attention".into(), true);
        }
        for w in waits {
            targets.push((rows.len(), Target::Wait(w.clone())));
            let run = w.index.map(|i| format!(" · run #{i}")).unwrap_or_default();
            let keys = match w.kind.as_str() {
                "tool_approval" => " — y approves · n denies",
                "ask_user" | "event" => " — Enter answers",
                _ => " — not answered here (open its run)",
            };
            push(
                &mut rows,
                format!("⚠ {}{run}{keys}", auto::wait_kind_label(&w.kind)),
                false,
            );
            if !w.prompt.is_empty() {
                wrap_into(&mut rows, "    ", &w.prompt, false);
            }
            if !w.choices.is_empty() {
                wrap_into(&mut rows, "    choices: ", &w.choices.join(" | "), true);
            }
            for call in auto::wait_tool_calls(w) {
                wrap_into(&mut rows, "    ", &call, false);
            }
        }
        for item in &s.attention.items {
            let kind = if item.kind == "failure" {
                "failed"
            } else {
                "notified"
            };
            push(
                &mut rows,
                format!("• run #{} {kind}: {}", item.index, item.title),
                false,
            );
            if !item.body.is_empty() {
                wrap_into(&mut rows, "    ", &item.body, true);
            }
        }
    }
    if d.next_cursor.is_some() {
        targets.push((rows.len(), Target::Earlier));
        push(&mut rows, "↑ load earlier occurrences (Enter)".into(), true);
    }
    if d.occurrences.is_empty() {
        push(&mut rows, "no runs yet — g runs it now".into(), true);
    }
    for o in &d.occurrences {
        targets.push((rows.len(), Target::Run(o.index)));
        let quiet = auto::occurrence_tone(o) == "quiet";
        push(&mut rows, auto::occurrence_header(o), quiet);
        if !o.user_turn.is_empty() {
            wrap_into(&mut rows, "  task:   ", &o.user_turn, true);
        }
        if !o.answer.is_empty() {
            wrap_into(&mut rows, "  answer: ", &o.answer, quiet);
        }
        if let Some((title, body)) = &o.notify {
            wrap_into(&mut rows, "  notify: ", &format!("{title} — {body}"), false);
        }
        if let Some(f) = &o.failure {
            wrap_into(
                &mut rows,
                "  failed: ",
                &format!(
                    "{} {} (after {} attempts)",
                    f.reason_code, f.message, f.attempts
                ),
                false,
            );
        }
        if !o.artifacts.is_empty() {
            wrap_into(&mut rows, "  artifacts: ", &o.artifacts.join(", "), true);
        }
    }
    (rows, targets)
}

/// Where the cursor rests before the user moves it (`usize::MAX`): on the
/// first wait that needs you, else on the newest run.
pub(crate) fn resolve_cursor(cursor: usize, targets: &[Target]) -> usize {
    if cursor != usize::MAX {
        return cursor.min(targets.len().saturating_sub(1));
    }
    targets
        .iter()
        .position(|t| matches!(t, Target::Wait(_)))
        .unwrap_or(targets.len().saturating_sub(1))
}

/// One automation: header, waits, runs, controls.
pub fn open_automation(cx: Scope, store: Store, ctx: &UiCtx, id: &str) {
    let id = id.to_string();
    store.automations.update(|v| v.open(&id));
    send(ctx, AutoCmd::Open { id: id.clone() });
    let ctx2 = ctx.clone();
    let size = modal_size(160, 40);
    // Rows are drawn indented inside the panel padding: wrap narrower than
    // the panel so no line is cut by the row ellipsis.
    let width = size.w - 12;
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        // Cursor over the targets; `usize::MAX` = the newest run (at rest).
        let cursor = mcx.signal(usize::MAX);
        let confirm_archive = mcx.signal(false);
        arm_poll(mcx, store, ctx2.clone());
        // Attention items shown here are acknowledged (the LAST displayed
        // item's cursor, once per cursor, only after the gateway accepts).
        {
            let ctx = ctx2.clone();
            mcx.effect(move || {
                if let Some((id, cursor)) = store.automations.with(|v| v.cursor_to_ack()) {
                    store.automations.update(|v| v.ack_sent(&id, &cursor));
                    send(&ctx, AutoCmd::Seen { id, cursor });
                }
            });
        }
        let targets = move || -> Vec<Target> {
            store.automations.with_untracked(|v| {
                v.detail
                    .as_ref()
                    .map(|d| detail_rows(d, width).1.into_iter().map(|(_, t)| t).collect())
                    .unwrap_or_default()
            })
        };
        let at = move || -> Option<Target> {
            let ts = targets();
            ts.get(resolve_cursor(cursor.get_untracked(), &ts)).cloned()
        };
        let summary = move || store.automations.with_untracked(|v| v.detail.as_ref().and_then(|d| d.summary.clone()));
        let move_cursor = move |delta: i64| {
            confirm_archive.set(false);
            let n = targets().len();
            if n == 0 {
                return;
            }
            let ts = targets();
            cursor.update(|c| {
                let base = resolve_cursor(*c, &ts) as i64;
                *c = (base + delta).clamp(0, n as i64 - 1) as usize;
            });
        };
        let act = {
            let ctx = ctx2.clone();
            move |control: Control| {
                confirm_archive.set(false);
                if let Some(s) = summary() {
                    command(store, &ctx, &s, control);
                }
            }
        };
        let switch = {
            let ctx = ctx2.clone();
            move || {
                confirm_archive.set(false);
                if let Some(s) = summary() {
                    switch_active(store, &ctx, &s);
                }
            }
        };
        let answer_wait = {
            let ctx = ctx2.clone();
            move |w: auto::Wait, answer: String| {
                let payload = match auto::wait_answer_payload(&w.kind, &answer) {
                    Ok(p) => p,
                    Err(e) => {
                        store.automations.update(|v| v.error = e);
                        return;
                    }
                };
                let key = format!("wait:{}:{}:{answer}", w.run_id, w.wait_key);
                let mut command_id = String::new();
                store.automations.update(|v| {
                    command_id = v.ids.id_for(&key, crate::config::mint_session_id);
                    v.busy = true;
                    v.error.clear();
                });
                send(
                    &ctx,
                    AutoCmd::Answer {
                        id: id.clone(),
                        command_id,
                        wait: Box::new(w),
                        payload,
                    },
                );
            }
        };
        let back = {
            let ctx = ctx2.clone();
            Rc::new(move || open_automations(cx, store, &ctx)) as Rc<dyn Fn()>
        };
        let reopen = {
            let ctx = ctx2.clone();
            let id = open_id(store).unwrap_or_default();
            Rc::new(move || open_automation(cx, store, &ctx, &id)) as Rc<dyn Fn()>
        };
        let enter = {
            let ctx = ctx2.clone();
            let answer_wait = answer_wait.clone();
            let reopen = reopen.clone();
            move || match at() {
                Some(Target::Earlier) => {
                    if let Some((id, c)) = store.automations.with_untracked(|v| {
                        v.detail.as_ref().and_then(|d| d.next_cursor.clone().map(|c| (d.id.clone(), c)))
                    }) {
                        send(&ctx, AutoCmd::More { id, cursor: c });
                    }
                }
                Some(Target::Wait(w)) if w.kind == "ask_user" || w.kind == "event" => {
                    let title = if w.kind == "ask_user" {
                        "answer the question".to_string()
                    } else {
                        "send the event payload (JSON)".to_string()
                    };
                    let mut info = vec![w.prompt.clone()];
                    if !w.choices.is_empty() {
                        info.push(format!("choices: {}", w.choices.join(" | ")));
                    }
                    let answer_wait = answer_wait.clone();
                    let reopen2 = reopen.clone();
                    let w2 = w.clone();
                    open_text(
                        cx,
                        &ctx,
                        title,
                        info,
                        w.choices.first().cloned().unwrap_or_default(),
                        Rc::new(move |text: String| {
                            answer_wait(w2.clone(), text);
                            reopen2();
                        }),
                        reopen.clone(),
                    );
                }
                Some(Target::Wait(w)) if w.kind == "tool_approval" => store
                    .automations
                    .update(|v| v.notice = "a tool approval is answered with y (approve) or n (deny)".into()),
                _ => {}
            }
        };
        let approve = {
            let answer_wait = answer_wait.clone();
            move |answer: &'static str| match at() {
                Some(Target::Wait(w)) if w.kind == "tool_approval" => answer_wait(w, answer.to_string()),
                _ => store
                    .automations
                    .update(|v| v.notice = "select an “Approval needed” row first (↑↓)".into()),
            }
        };
        let discuss = {
            let ctx = ctx2.clone();
            let reopen = reopen.clone();
            move || {
                let Some(s) = summary() else { return };
                if let Err(why) = auto::control_state(&s, Control::Discuss, false) {
                    store.automations.update(|v| v.error = format!("discuss: {why}"));
                    return;
                }
                // Discuss switches THIS terminal to the new chat; a switch
                // cancels a run in progress here, so it is refused instead.
                if store.phase.get_untracked() != crate::store::Phase::Idle {
                    store.automations.update(|v| {
                        v.error = "discuss: a run is in progress in this session — Discuss switches this terminal to a new chat; wait for it or /cancel it first".into()
                    });
                    return;
                }
                let run = match at() {
                    Some(Target::Run(i)) => i,
                    _ => {
                        store.automations.update(|v| v.notice = "select a run (↑↓) to discuss".into());
                        return;
                    }
                };
                let finished = store.automations.with_untracked(|v| {
                    v.detail
                        .as_ref()
                        .and_then(|d| d.occurrences.iter().find(|o| o.index == run))
                        .is_some_and(auto::can_discuss)
                });
                if !finished {
                    store
                        .automations
                        .update(|v| v.error = format!("discuss: run #{run} has not finished yet"));
                    return;
                }
                let sid = s.id.clone();
                open_text(
                    cx,
                    &ctx,
                    format!("discuss run #{run} of “{}” — a new chat, in place", s.title),
                    vec![auto::DISCUSS_HELP.to_string()],
                    String::new(),
                    {
                        let ctx = ctx.clone();
                        Rc::new(move |prompt: String| {
                            if prompt.trim().is_empty() {
                                store.automations.update(|v| v.error = "discuss: write your first message".into());
                                return;
                            }
                            let key = format!("discuss:{sid}:{run}:{prompt}");
                            let mut request_id = String::new();
                            store.automations.update(|v| {
                                request_id = v.ids.id_for(&key, crate::config::mint_session_id);
                                v.busy = true;
                                v.error.clear();
                                v.notice = format!("starting the discussion of run #{run}…");
                            });
                            send(
                                &ctx,
                                AutoCmd::Discuss {
                                    id: sid.clone(),
                                    request_id,
                                    index: run,
                                    prompt,
                                },
                            );
                        })
                    },
                    reopen.clone(),
                );
            }
        };
        // Edit opens the settings panels on this automation (the Code web
        // header's Edit → `openAutomationSettings`): each change there saves
        // a new revision.
        let revise = {
            let ctx = ctx2.clone();
            move || {
                let Some(s) = summary() else { return };
                if let Err(why) = auto::control_state(&s, Control::Revise, false) {
                    store.automations.update(|v| v.error = format!("Edit: {why}"));
                    return;
                }
                crate::ui::rail_view::open_automation_settings(cx, store, &ctx, &s.id);
            }
        };
        let archive = move || {
            let Some(s) = summary() else { return };
            if let Err(why) = auto::control_state(&s, Control::Archive, false) {
                store.automations.update(|v| v.error = format!("Archive: {why}"));
            } else {
                store.automations.update(|v| v.error.clear());
                confirm_archive.set(true);
            }
        };
        let confirm_yes = {
            let ctx = ctx2.clone();
            move || {
                confirm_archive.set(false);
                if let Some(s) = summary() {
                    command(store, &ctx, &s, Control::Archive);
                }
            }
        };
        let unarchive = {
            let ctx = ctx2.clone();
            move || {
                confirm_archive.set(false);
                if let Some(s) = summary() {
                    command(store, &ctx, &s, Control::Unarchive);
                }
            }
        };
        let folder = {
            let ctx = ctx2.clone();
            let reopen = reopen.clone();
            move || {
                let Some(s) = summary() else { return };
                if s.workspace_root.is_none() {
                    store.automations.update(|v| v.error = "this automation has no folder the gateway reports".into());
                    return;
                }
                // The automation id IS its controller run id; the gateway's
                // run workspace routes serve the automation's folder.
                crate::ui::modals::open_run_files(
                    cx,
                    store,
                    &ctx,
                    s.id.clone(),
                    format!("automation folder — “{}” on the gateway", s.title),
                    Some(reopen.clone()),
                );
            }
        };
        let key = |c: char| KeyChord::plain(Key::Char(c));
        Element::new()
            .style(LayoutStyle::column().gap(1).padding(Edges::all(1)))
            .focusable()
            .autofocus()
            .shortcut(KeyChord::plain(Key::Escape), move |_| {
                if confirm_archive.get_untracked() {
                    confirm_archive.set(false);
                } else {
                    back()
                }
            })
            .shortcut(KeyChord::plain(Key::Up), move |_| move_cursor(-1))
            .shortcut(KeyChord::plain(Key::Down), move |_| move_cursor(1))
            .shortcut(KeyChord::plain(Key::Enter), move |_| enter())
            // While the Archive question shows, y/n answer IT (inline
            // confirmation); otherwise they approve/deny a tool call.
            .shortcut(key('y'), {
                let approve = approve.clone();
                move |_| {
                    if confirm_archive.get_untracked() {
                        confirm_yes()
                    } else {
                        approve("approve")
                    }
                }
            })
            .shortcut(key('n'), move |_| {
                if confirm_archive.get_untracked() {
                    confirm_archive.set(false)
                } else {
                    approve("deny")
                }
            })
            .shortcut(key('u'), move |_| unarchive())
            .shortcut(KeyChord::plain(Key::Char(' ')), {
                let switch = switch.clone();
                move |_| switch()
            })
            // `p` (the old pause/resume key) switches Active too.
            .shortcut(key('p'), move |_| switch())
            .shortcut(key('g'), {
                let act = act.clone();
                move |_| act(Control::RunNow)
            })
            .shortcut(key('x'), {
                let act = act.clone();
                move |_| act(Control::StopCurrent)
            })
            .shortcut(key('e'), move |_| revise())
            .shortcut(key('a'), move |_| archive())
            .shortcut(key('d'), move |_| discuss())
            .shortcut(key('w'), move |_| folder())
            .shortcut(key('r'), {
                let ctx = ctx2.clone();
                move |_| refresh(store, &ctx)
            })
            .child(dyn_view(LayoutStyle::line(1).shrink(0.0), move || {
                let t2 = abstracttui::app::current_theme().tokens;
                let title = store.automations.with(|v| {
                    v.detail
                        .as_ref()
                        .and_then(|d| d.summary.as_ref())
                        .map(|s| format!("Automations / {}", s.title))
                        .unwrap_or_else(|| "Automations / Loading automation…".into())
                });
                title_row(&t2, title)
            }))
            .child(dyn_view(LayoutStyle::line(1).shrink(0.0), move || {
                let row = store.automations.with(|v| {
                    v.detail
                        .as_ref()
                        .and_then(|d| d.summary.as_ref())
                        .map(|s| active_row(s, v.busy))
                });
                draw_rows(row.into_iter().collect(), 0, Vec::new())
            }))
            .child(dyn_view(LayoutStyle::column().shrink(0.0), move || {
                let t2 = abstracttui::app::current_theme().tokens;
                let now = auto::now_unix();
                let mut lines = store.automations.with(|v| {
                    let mut lines = match v.detail.as_ref() {
                        Some(d) => match &d.summary {
                            Some(s) => header_lines(s, d.definition.as_ref(), now),
                            None => vec!["Loading automation…".into()],
                        },
                        None => Vec::new(),
                    };
                    if confirm_archive.get() {
                        if let Some(s) = v.detail.as_ref().and_then(|d| d.summary.as_ref()) {
                            lines.push(format!("{}  y Archive · n Keep it", archive_question(&s.title)));
                        }
                    }
                    if let Some(e) = v.detail.as_ref().map(|d| d.error.clone()).filter(|e| !e.is_empty()) {
                        lines.push(format!("could not read it: {e}"));
                    }
                    lines
                });
                lines.extend(store.automations.with(status_lines));
                note_lines(&t2, &lines, 8)
            }))
            .child(dyn_view(
                LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),
                move || {
                    let (rows, targets) = store.automations.with(|v| {
                        v.detail.as_ref().map(|d| detail_rows(d, width)).unwrap_or_default()
                    });
                    let ts: Vec<Target> = targets.iter().map(|(_, t)| t.clone()).collect();
                    let cur = resolve_cursor(cursor.get(), &ts);
                    let selectable = targets.into_iter().map(|(row, _)| row).collect();
                    draw_rows(rows, cur, selectable)
                },
            ))
            .child(hint_bar(&t, DETAIL_HINTS, 8))
            .child(note_lines(&t, &[auto::run_now_key_line()], 8))
            .build()
    });
}

// ---------------------------------------------------------------------------
// Text input (discuss prompt, answers, revise fields, the task)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub(crate) fn open_text(
    cx: Scope,
    ctx: &UiCtx,
    title: String,
    info: Vec<String>,
    initial: String,
    on_submit: Rc<dyn Fn(String)>,
    on_cancel: Rc<dyn Fn()>,
) {
    let panel = modal_size(100, 10);
    let text_w = (panel.w - 5).max(8);
    let lines: Vec<String> = info
        .iter()
        .filter(|l| !l.is_empty())
        .flat_map(|l| wrap_lines(l, text_w, None))
        .collect();
    // panel padding 2 + content padding 2 + title 1 + input 1 + hint 1 +
    // gaps 3 = 10 fixed rows, plus the info lines.
    let size = modal_size(100, 10 + lines.len() as i32);
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let value = mcx.signal(initial.clone());
        let mut col = Element::new()
            .style(LayoutStyle::column().gap(1).padding(Edges::all(1)))
            .shortcut(KeyChord::plain(Key::Escape), {
                let on_cancel = on_cancel.clone();
                move |_| on_cancel()
            })
            .child(title_row(&t, title.clone()));
        let mut body =
            Element::new().style(LayoutStyle::column().grow(1.0).basis(Dimension::Cells(0)));
        for line in &lines {
            body = body.child(hint_row(&t, line.clone()));
        }
        col = col.child(body.build());
        col.child(
            TextInput::new()
                .value(value)
                .placeholder("type, then Enter")
                .placeholder_while_focused(true)
                .on_submit({
                    let on_submit = on_submit.clone();
                    move |text| on_submit(text.to_string())
                })
                .layout(LayoutStyle::line(1).shrink(0.0))
                .element(mcx, &t)
                .autofocus()
                .build(),
        )
        .child(hint_row(
            &t,
            "Enter sends · Home/End move to the start/end · Esc goes back".into(),
        ))
        .build()
    });
}

// ---------------------------------------------------------------------------

/// The last task typed in this conversation (the default task to schedule).
fn last_prompt(store: Store) -> String {
    store.fold.with_untracked(|f| {
        f.items
            .iter()
            .rev()
            .find_map(|i| match i {
                Item::User { text } => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_default()
    })
}

/// `/schedule [task]` — create an automation that runs the CURRENT workflow.
pub fn open_schedule(cx: Scope, store: Store, ctx: &UiCtx, seed: Option<String>) {
    let workflow = store.workflow.get_untracked();
    let Some(target) = auto::target_for(&workflow) else {
        store.notify("/schedule runs the current workflow — pick one first (/workflow)");
        return;
    };
    let seed = seed.unwrap_or_else(|| last_prompt(store));
    let what = if workflow.gateway_default {
        format!("the gateway default agent ({})", workflow.versioned_label())
    } else {
        workflow.versioned_label()
    };
    let ctx2 = ctx.clone();
    let cancel: Rc<dyn Fn()> = {
        let ctx = ctx.clone();
        Rc::new(move || ctx.close_modal())
    };
    open_text(
        cx,
        ctx,
        "new automation — 1/4 the task".into(),
        vec![
            format!("Runs {what} on a schedule, on the gateway (every client sees it)."),
            "The task below is sent as the prompt of every run.".into(),
        ],
        seed,
        Rc::new(move |prompt: String| {
            if prompt.trim().is_empty() {
                store.notify("an automation needs a task — write what it should do");
                return;
            }
            let form = auto::CreateForm {
                prompt,
                ..auto::CreateForm::default()
            };
            schedule_when(cx, store, &ctx2, form, target.clone());
        }),
        cancel,
    );
}

/// The When step's rows: the kit's presets, then custom interval / once.
pub const WHEN_PRESETS: [(&str, &str, char); 6] = [
    ("every 5 minutes", "5", 'm'),
    ("every 30 minutes", "30", 'm'),
    ("every hour", "1", 'h'),
    ("every 8 hours", "8", 'h'),
    ("every 24 hours", "24", 'h'),
    ("every 7 days", "7", 'd'),
];

fn schedule_when(
    cx: Scope,
    store: Store,
    ctx: &UiCtx,
    form: auto::CreateForm,
    target: serde_json::Value,
) {
    let mut labels: Vec<String> = WHEN_PRESETS
        .iter()
        .map(|(l, _, _)| format!("{l} (UTC)"))
        .collect();
    labels.push("every … (type an interval: 90m, 12h, 3d)".into());
    labels.push("once, at a date and time (UTC)".into());
    let ctx2 = ctx.clone();
    let cancel_ctx = ctx.clone();
    open_picker(
        cx,
        ctx,
        Picker {
            title: "new automation — 2/4 when (fixed UTC intervals)".into(),
            labels,
            live: None,
            start: 4,
            size: modal_size(80, 8 + 9),
            hint: Some("Enter chooses · Esc cancels".into()),
            live_hint: None,
            keys: Vec::new(),
            on_mount: None,
            on_selection: None,
            on_choose: Box::new(move |ix| {
                let mut form = form.clone();
                let target = target.clone();
                if let Some((_, n, u)) = WHEN_PRESETS.get(ix) {
                    form.when = auto::When::Every {
                        amount: n.to_string(),
                        unit: *u,
                    };
                    return schedule_context(cx, store, &ctx2, form, target);
                }
                let once = ix == WHEN_PRESETS.len() + 1;
                let ctx3 = ctx2.clone();
                let cancel: Rc<dyn Fn()> = {
                    let ctx = ctx2.clone();
                    Rc::new(move || ctx.close_modal())
                };
                open_text(
                    cx,
                    &ctx2,
                    if once {
                        "new automation — 2/4 once at (UTC)".into()
                    } else {
                        "new automation — 2/4 every (UTC)".into()
                    },
                    vec![if once {
                        "A date and time read as UTC: YYYY-MM-DD HH:MM".into()
                    } else {
                        "A whole number followed by m, h or d: 90m, 12h, 3d.".into()
                    }],
                    String::new(),
                    Rc::new(move |text: String| {
                        let mut form = form.clone();
                        let text = text.trim().to_string();
                        form.when = if once {
                            auto::When::Once { at: text }
                        } else {
                            let unit = text.chars().last().unwrap_or('h');
                            auto::When::Every {
                                amount: text[..text.len().saturating_sub(unit.len_utf8())]
                                    .to_string(),
                                unit,
                            }
                        };
                        if auto::schedule_preview(&form).is_empty() {
                            store.notify(if once {
                                "that is not a date and time (UTC) — write YYYY-MM-DD HH:MM"
                            } else {
                                "that is not an interval — write a whole number and m, h or d (90m, 12h, 3d)"
                            });
                            return;
                        }
                        schedule_context(cx, store, &ctx3, form, target.clone());
                    }),
                    cancel,
                );
            }),
            on_cancel: Some(Box::new(move || cancel_ctx.close_modal())),
        },
    );
}

fn schedule_context(
    cx: Scope,
    store: Store,
    ctx: &UiCtx,
    form: auto::CreateForm,
    target: serde_json::Value,
) {
    let ctx2 = ctx.clone();
    let cancel_ctx = ctx.clone();
    let preview = auto::schedule_preview(&form);
    open_picker(
        cx,
        ctx,
        Picker {
            title: format!("new automation — 3/4 context · {preview}"),
            labels: vec![
                auto::context_label("independent").to_string(),
                auto::context_label("growing").to_string(),
            ],
            live: None,
            start: 0,
            size: modal_size(90, 2 + 9),
            hint: Some("Growing replays the previous runs as history, within the gateway's context window · Esc cancels".into()),
            live_hint: None,
            keys: Vec::new(),
            on_mount: None,
            on_selection: None,
            on_choose: Box::new(move |ix| {
                let mut form = form.clone();
                form.context = if ix == 1 { "growing" } else { "independent" }.into();
                schedule_tools(cx, store, &ctx2, form, target.clone());
            }),
            on_cancel: Some(Box::new(move || cancel_ctx.close_modal())),
        },
    );
}

fn schedule_tools(
    cx: Scope,
    store: Store,
    ctx: &UiCtx,
    form: auto::CreateForm,
    target: serde_json::Value,
) {
    let ctx2 = ctx.clone();
    let cancel_ctx = ctx.clone();
    open_picker(
        cx,
        ctx,
        Picker {
            title: "new automation — 4/4 tools (Enter creates it)".into(),
            labels: vec![
                format!(
                    "Run without asking — {}",
                    auto::TOOL_APPROVAL_CONSENT.to_lowercase()
                ),
                "Ask me before each tool call (every tool call waits for approval in /automations)"
                    .into(),
            ],
            live: None,
            start: 0,
            size: modal_size(110, 2 + 9),
            hint: Some("Questions the workflow asks always wait for you · Esc cancels".into()),
            live_hint: None,
            keys: Vec::new(),
            on_mount: None,
            on_selection: None,
            on_choose: Box::new(move |ix| {
                let mut form = form.clone();
                form.tool_approval = if ix == 1 { "ask" } else { "auto" }.into();
                // One request id per distinct body: a retry of the same body
                // after a transport failure is answered idempotently.
                let probe = auto::build_create_request(&form, Some(target.clone()), "");
                match probe {
                    Err(errors) => store.notify(format!("not created: {}", errors.join(" "))),
                    Ok(body) => {
                        let key = format!("create:{body}");
                        let mut request_id = String::new();
                        store.automations.update(|v| {
                            request_id = v.ids.id_for(&key, crate::config::mint_session_id);
                            v.busy = true;
                            v.error.clear();
                            v.notice = "creating the automation…".into();
                        });
                        let mut body = body;
                        body["request_id"] = serde_json::json!(request_id);
                        send(&ctx2, AutoCmd::Create { body });
                        // The list opens now; the new automation opens when
                        // the gateway answers (`wire_automations`).
                        open_automations(cx, store, &ctx2);
                    }
                }
            }),
            on_cancel: Some(Box::new(move || cancel_ctx.close_modal())),
        },
    );
}

// ---------------------------------------------------------------------------
// Root wiring: a started discussion becomes THIS terminal's chat; a created
// automation opens.
// ---------------------------------------------------------------------------

pub fn wire_automations(cx: Scope, store: Store, ctx: UiCtx) {
    cx.effect(move || {
        let (discussion, created) = store
            .automations
            .with(|v| (v.discussion.clone(), v.created.clone()));
        if let Some((index, d)) = discussion {
            store.automations.update(|v| v.discussion = None);
            ctx.close_modal();
            crate::ui::switch_session(store, &ctx, &d.session_id);
            store.notify(auto::discussion_notice(index, &d));
        } else if let Some(id) = created {
            store.automations.update(|v| v.created = None);
            if ctx.modal_open() {
                open_automation(cx, store, &ctx, &id);
            }
        }
    });
}
