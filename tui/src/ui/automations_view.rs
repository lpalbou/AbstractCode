//! `/automations` and `/automation`: the gateway's automations in the terminal.
//!
//! Same behaviour and contract as the Observer, the Assistant and the
//! ui-kit panel (`crate::automations` holds the shared rules):
//! - the list: state as text + icon ("Active ▶"), cadence, what runs NOW
//!   (only `current_occurrence`), the NEXT run (only the served
//!   `next_run_at` / `next_run_local`; a calendar rule reads as the served
//!   `schedule_rule_text`),
//!   attention; archived rows hidden until asked (`h`);
//! - one automation: its header (the folder is `workspace_root`, browsed
//!   through the gateway's workspace routes with `w`), the waits that need
//!   you (tool approvals `y`/`n`, questions and events with Enter), its
//!   runs as chat pairs, and every control (pause/resume, run now, stop
//!   current, revise, archive — which hides and stops, never deletes);
//! - Discuss (`d`) forks the automation at the selected run and SWITCHES
//!   this terminal to the new chat, in place (one session pool);
//! - `/automation` ("New automation"; `/schedule` is its old name) opens
//!   the kit's schedule dialog (`crate::ui::schedule_view`):
//!   seven steps with the same `schedule@2` body as every client; When =
//!   Repeat · Daily · Weekly · Monthly · Once at… (each worded by the
//!   gateway: schedule-preview's `first_run_sentence`, + the time-zone line
//!   for the wall-clock kinds) or "When an email arrives".
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
use crate::ui::cards::{hint_bar, note_lines, Card, CardLine, Ink};
use crate::ui::modals::{hint_row, modal_size, title_row, wrap_lines, Mark, RowSpec};
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
        // Busy + the pending mark on this automation's row until the
        // gateway's answer shows it done (or refuses).
        v.start_pending(s, control);
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
/// automation screen's first header row. `pending` = the control in flight
/// on it: a switch flip shows `[…] Active — Pausing…` until the answer.
pub(crate) fn active_row(s: &Summary, busy: bool, pending: Option<Control>) -> RowSpec {
    let detail = match pending {
        Some(c @ (Control::Pause | Control::Resume)) => c.busy_notice().to_string(),
        _ => auto::active_detail(s, busy),
    };
    RowSpec {
        text: format!("{} — {detail}", auto::active_label()),
        header: false,
        checked: Some(active_mark(s, busy, pending)),
        dim: false,
    }
}

/// The switch's mark: on/off, pending (`[…]`) while a flip is in flight,
/// or unavailable (with the state kept while another command is in flight).
fn active_mark(s: &Summary, busy: bool, pending: Option<Control>) -> Mark {
    if matches!(pending, Some(Control::Pause | Control::Resume)) {
        return Mark::Pending;
    }
    match auto::active_switch(s, busy) {
        Ok(on) => Mark::switch(on),
        // In flight: keep showing the current state, not "unavailable".
        Err(_) if busy && !s.legacy && (s.status == "active" || s.status == "paused") => {
            Mark::switch(s.status == "active")
        }
        Err(_) => Mark::Unavailable,
    }
}

/// `[x] Active` / `[ ] Active` / `[-] Active` — the card's switch;
/// `[…] Active` while a flip is in flight; another action in flight on
/// this automation reads as its own words ("Starting a run…").
pub(crate) fn active_switch_text(s: &Summary, busy: bool, pending: Option<Control>) -> String {
    match pending {
        Some(
            c @ (Control::RunNow | Control::StopCurrent | Control::Archive | Control::Unarchive),
        ) => c.busy_notice().to_string(),
        _ => format!(
            "{}{}",
            active_mark(s, busy, pending).marker(),
            auto::active_label()
        ),
    }
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
    /// The `Paused · N` line (Enter opens/closes it; ADR 0048).
    PausedLine,
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
/// `next in 20 h` with the Active switch at the right (+ a third line, in the
/// error ink, only while the last "Email result" failed); then the quiet
/// `Paused · N` line (ADR 0048; when open, the paused ones as the same
/// cards), then the quiet `Archived · N` line and, when open, the archived ones with Unarchive.
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
            // ADR 0048: the gateway's order; the paused ones fold under `Paused · N`.
            let (shown, paused) = auto::sections(items);
            let push_card = |s: &Summary, cards: &mut Vec<Card>, targets: &mut Vec<ListTarget>| {
                let (line1, line2) = auto::card_lines(s, now);
                let mut lines = vec![CardLine::new(s.title.clone(), Ink::Title)
                    .right(auto::waiting_badge(s).unwrap_or(""))];
                lines.push(CardLine::new(line1, Ink::Faint).indent(2));
                let pending = v.pending_on(&s.id);
                let switch_ink = match (pending, active_mark(s, v.busy, pending)) {
                    (Some(_), _) => Ink::Accent,
                    (_, Mark::On) => Ink::On,
                    (_, Mark::Unavailable) => Ink::Faint,
                    _ => Ink::Text,
                };
                lines.push(
                    CardLine::new(line2, Ink::Faint)
                        .indent(2)
                        .right(active_switch_text(s, v.busy, pending))
                        .right_ink(switch_ink),
                );
                // Round 16: a third line only while the last "Email result" failed — the
                // gateway's `last_notification.text` verbatim + "· 3 h ago".
                if let Some(failed) = auto::email_failed_line(s, now) {
                    lines.push(CardLine::new(failed, Ink::Error).indent(2));
                }
                if confirm == Some(s.id.as_str()) {
                    lines.push(CardLine::new(archive_question(&s.title), Ink::Accent).indent(2));
                    lines.push(CardLine::new("y Archive · n Keep it", Ink::Accent).indent(2));
                }
                cards.push(Card::new(lines));
                targets.push(ListTarget::Automation(s.id.clone()));
            };
            for s in &shown {
                push_card(s, &mut cards, &mut targets);
            }
            // The gateway's figure when it serves one (ADR 0048), else the rows read.
            let paused_n = match (paused.is_empty(), v.paused_count) {
                (false, Some(n)) => n as usize,
                _ => paused.len(),
            };
            if let Some(line) = auto::paused_line(paused_n) {
                cards.push(Card::new(vec![CardLine::new(line, Ink::Faint)]));
                targets.push(ListTarget::PausedLine);
                if v.paused_open {
                    for s in &paused {
                        push_card(s, &mut cards, &mut targets);
                    }
                }
            }
            if shown.is_empty() && paused.is_empty() {
                cards.push(Card::note(
                    "No automations yet. n creates one from the current workflow (/automation).",
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
                        let right = match v.pending_on(&s.id) {
                            Some(c) => c.busy_notice(),
                            None => Control::Unarchive.button(),
                        };
                        cards.push(Card::new(vec![CardLine::new(s.title.clone(), Ink::Faint)
                            .indent(2)
                            .right(right)]));
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
        // The list's window: keys keep the cursor in view, the wheel
        // scrolls it (anywhere on the screen).
        let scroll = crate::ui::cards::CardScroll::new(mcx);
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
        let follow = scroll.clone();
        let move_cursor = move |delta: i64| {
            confirm_archive.set(None);
            follow.follow_cursor();
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
                Some(ListTarget::PausedLine) => {
                    store.automations.update(|v| v.paused_open = !v.paused_open);
                }
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
        let cards_scroll = scroll.clone();
        Element::new()
            .style(LayoutStyle::column().padding(Edges::all(1)))
            .focusable()
            .autofocus()
            .on(abstracttui::ui::Phase::Bubble, scroll.on_wheel())
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
            .shortcut(KeyChord::plain(Key::Up), {
                let move_cursor = move_cursor.clone();
                move |_| move_cursor(-1)
            })
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
                    crate::ui::cards::draw_cards_wheel(cards, cur, &cards_scroll)
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

/// The automation's runs as a conversation, oldest first: per run a
/// separator (`#N · completed · <time>`, the run header's words, feed key
/// `r<N>`), its task as your turn, its answer as the reply, then what it
/// notified, its failure and its artifacts.
pub fn occurrences_as_chat(
    occs: &[auto::Occurrence],
) -> Vec<crate::ui::transcript_view::ChatEntry> {
    use crate::transcript::Item;
    use crate::ui::transcript_view::ChatEntry;
    let mut out = Vec::new();
    let mut ordered: Vec<&auto::Occurrence> = occs.iter().collect();
    ordered.sort_by_key(|o| o.index);
    for o in ordered {
        out.push(ChatEntry::Separator {
            key: format!("r{}", o.index),
            label: auto::occurrence_header(o),
        });
        if !o.user_turn.is_empty() {
            out.push(ChatEntry::Item(Box::new(Item::User {
                text: o.user_turn.clone(),
            })));
        }
        if !o.answer.is_empty() {
            out.push(ChatEntry::Item(Box::new(Item::Assistant {
                text: o.answer.clone(),
                final_answer: true,
                author: None,
            })));
        }
        if let Some((title, body)) = &o.notify {
            out.push(ChatEntry::Item(Box::new(Item::Info {
                text: format!("notify: {title} — {body}"),
            })));
        }
        if let Some(f) = &o.failure {
            out.push(ChatEntry::Item(Box::new(Item::Error {
                text: format!(
                    "failed: {} {} (after {} attempts)",
                    f.reason_code, f.message, f.attempts
                ),
            })));
        }
        if !o.artifacts.is_empty() {
            out.push(ChatEntry::Item(Box::new(Item::Info {
                text: format!("artifacts: {}", o.artifacts.join(", ")),
            })));
        }
    }
    out
}

/// The "Open as chat" view's key hints.
pub const CHAT_HINTS: &[(&str, &str)] = &[
    ("↑↓ PgUp PgDn Home End wheel", "scroll"),
    ("Esc", "back to the automation"),
];

/// "Open as chat": the automation's loaded runs in the conversation's
/// transcript widget (read-only, scrollable, wrapped), opened on run
/// `at` (else the newest). Esc goes back to the automation; `d` Discuss
/// there still forks a run into a new chat.
pub fn open_as_chat(cx: Scope, store: Store, ctx: &UiCtx, at: Option<u64>) {
    let Some((id, title, entries)) = store.automations.with_untracked(|v| {
        v.detail.as_ref().map(|d| {
            (
                d.id.clone(),
                d.summary
                    .as_ref()
                    .map(|s| s.title.clone())
                    .unwrap_or_default(),
                occurrences_as_chat(&d.occurrences),
            )
        })
    }) else {
        return;
    };
    let n = entries
        .iter()
        .filter(|e| matches!(e, crate::ui::transcript_view::ChatEntry::Separator { .. }))
        .count();
    let ctx2 = ctx.clone();
    let size = modal_size(160, 44);
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let back = {
            let ctx = ctx2.clone();
            let id = id.clone();
            move || open_automation(cx, store, &ctx, &id)
        };
        let body = if entries.is_empty() {
            note_lines(&t, &["no runs yet — g runs it now".to_string()], 8)
        } else {
            crate::ui::transcript_view::read_only_transcript(
                mcx,
                store,
                &entries,
                at.map(|i| format!("r{i}")),
            )
        };
        Element::new()
            .style(LayoutStyle::column().gap(1).padding(Edges::all(1)))
            .shortcut(KeyChord::plain(Key::Escape), move |_| back())
            .child(title_row(
                &t,
                format!(
                    "Automations / {title} · as chat · {n} {} (read-only)",
                    if n == 1 { "run" } else { "runs" }
                ),
            ))
            .child(
                Element::new()
                    .style(LayoutStyle::column().grow(1.0).basis(Dimension::Cells(0)))
                    .child(body)
                    .build(),
            )
            .child(hint_bar(&t, CHAT_HINTS, 8))
            .build()
    });
}

/// What Ctrl+P reads in one automation (R17.1): the SELECTED run's reply,
/// spoken through the automation's run (the web's `runId: automationId`).
/// `Err` = the sentence to show instead.
pub fn read_aloud_target(
    d: &auto::Detail,
    at: Option<&Target>,
) -> Result<(String, String), String> {
    let Some(Target::Run(index)) = at else {
        return Err("select a run (↑↓) to read its reply aloud".into());
    };
    let reply = d
        .occurrences
        .iter()
        .find(|o| o.index == *index)
        .map(|o| o.answer.trim().to_string())
        .unwrap_or_default();
    if reply.is_empty() {
        return Err("No reply to read aloud yet.".into());
    }
    Ok((d.id.clone(), reply))
}

/// One automation (operator 2026-11-08, "make it readable"): panels built
/// by [`crate::ui::automation_detail`] — the header and its Details, the
/// runs table and the selected row's output — and the wheel over every
/// part of the screen: over the runs it scrolls the runs, anywhere else
/// it scrolls the output (the screen's main scroll region).
pub fn open_automation(cx: Scope, store: Store, ctx: &UiCtx, id: &str) {
    use crate::ui::automation_detail as panels;
    let id = id.to_string();
    store.automations.update(|v| v.open(&id));
    send(ctx, AutoCmd::Open { id: id.clone() });
    let ctx2 = ctx.clone();
    let size = modal_size(170, 60);
    // Side by side from this width; narrower terminals stack the panels
    // and move the Details under the output.
    let wide = size.w >= 100;
    // The content box: the modal's frame and this column's padding.
    let inner_w = size.w - 4;
    let inner_h = size.h - 4;
    // Narrow: the runs panel's tallest (its border, header row and rows);
    // a short terminal keeps two rows of runs so the output keeps rows too.
    // (Narrow panels are frameless: a title line, the table's header row.)
    let runs_max_h = if inner_h >= 24 { 7 } else { 4 };
    // ...and the output's least: its title and five lines, two when short
    // (the hints give up their rows first).
    let out_min_h = if inner_h >= 20 { 6 } else { 3 };
    ctx.open_modal(cx, size, move |mcx| {
        // Cursor over the targets; `usize::MAX` = at rest (the first wait
        // that needs you, else the newest run).
        let cursor = mcx.signal(usize::MAX);
        let confirm_archive = mcx.signal(false);
        // The output panel's scroll: offset, measured extent, viewport.
        let out_off = mcx.signal(0i32);
        let out_ext = mcx.signal((0i32, 0i32));
        let out_vp = mcx.signal((0i32, 0i32));
        // The runs list's scroll and its "bring this row into view".
        let list_off = mcx.signal(0i32);
        let list_to = mcx.signal(Option::<usize>::None);
        // `l`: the selected run's artifacts listed instead of counted.
        let artifacts_open = mcx.signal(false);
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
                v.detail.as_ref().map(panels::targets).unwrap_or_default()
            })
        };
        let at = move || -> Option<Target> {
            let ts = targets();
            ts.get(panels::resolve_cursor(cursor.get_untracked(), &ts)).cloned()
        };
        let summary = move || store.automations.with_untracked(|v| v.detail.as_ref().and_then(|d| d.summary.clone()));
        // The output scrolls by `delta` rows, clamped to what it measured.
        let scroll_output = move |delta: i32| {
            let max = (out_ext.get_untracked().1 - out_vp.get_untracked().1).max(0);
            out_off.update(|o| *o = o.saturating_add(delta).clamp(0, max));
        };
        let page = move || (out_vp.get_untracked().1 - 2).max(1);
        let move_cursor = move |delta: i64| {
            confirm_archive.set(false);
            let ts = targets();
            let n = ts.len();
            if n == 0 {
                return;
            }
            let next = (panels::resolve_cursor(cursor.get_untracked(), &ts) as i64 + delta)
                .clamp(0, n as i64 - 1) as usize;
            cursor.set(next);
            out_off.set(0);
            list_to.set(Some(next));
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
                Some(Target::Run(i)) => open_as_chat(cx, store, &ctx, Some(i)),
                _ => {}
            }
        };
        let as_chat = {
            let ctx = ctx2.clone();
            move || {
                let at = match at() {
                    Some(Target::Run(i)) => Some(i),
                    _ => None,
                };
                open_as_chat(cx, store, &ctx, at)
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
        // The selection and the output's scroll keys go FIRST (capture): a
        // click focuses the runs list or the output, and their own arrow
        // and page keys must not take these over.
        let nav = move |ectx: &mut abstracttui::ui::EventCtx, ev: &abstracttui::ui::UiEvent| {
            // A press keeps the keyboard HERE (the press still reaches the
            // row or the scrollbar under it): the runs list is rebuilt when
            // its selection moves, and a focused list that is rebuilt would
            // leave the keys with nobody — Esc and every key dead.
            if let abstracttui::ui::UiEvent::Mouse(m) = ev {
                if matches!(m.kind, abstracttui::ui::MouseKind::Down(_)) {
                    if let Some(me) = ectx.current() {
                        ectx.request_focus(me);
                    }
                }
                return;
            }
            let abstracttui::ui::UiEvent::Key(k) = ev else {
                return;
            };
            if k.mods != Mods::NONE {
                return;
            }
            match k.key {
                Key::Up => move_cursor(-1),
                Key::Down => move_cursor(1),
                Key::PageUp => scroll_output(-page()),
                Key::PageDown => scroll_output(page()),
                Key::Home => out_off.set(0),
                Key::End => scroll_output(i32::MAX / 2),
                _ => return,
            }
            ectx.stop_propagation();
        };
        // The wheel anywhere the runs list or the output did not take it
        // (the title, the header panels, the hints, the borders): the
        // output scrolls — the screen's main scroll region (the AbstractTUI
        // modal rule, which the published engine does not apply yet).
        let wheel = move |ectx: &mut abstracttui::ui::EventCtx, ev: &abstracttui::ui::UiEvent| {
            if let Some(delta) = wheel_delta(ev) {
                scroll_output(delta);
                ectx.stop_propagation();
            }
        };
        // Over the runs panel's border and header, and at the list's ends:
        // the runs scroll (clamped), never the output.
        let runs_wheel = move |ectx: &mut abstracttui::ui::EventCtx, ev: &abstracttui::ui::UiEvent| {
            let Some(delta) = wheel_delta(ev) else {
                return;
            };
            let rows = targets().len() as i32;
            // The panel's border (2; narrow: its title line, 1) and the
            // table's header row (1).
            let view_h = (ectx.current_rect().h - if wide { 3 } else { 2 }).max(1);
            list_off.update(|o| *o = (*o + delta).clamp(0, (rows - view_h).max(0)));
            ectx.stop_propagation();
        };
        let toggle_artifacts = move || artifacts_open.update(|o| *o = !*o);
        // The panel beside (or under) the runs: the selected row's title,
        // its inked lines, then its document in a Scroll.
        let output_panel = move |scx: Scope, grow: bool| -> View {
            let open = artifacts_open.get();
            let out = store.automations.with(|v| {
                let Some(d) = v.detail.as_ref() else {
                    return panels::output(&auto::Detail::default(), None, open, None);
                };
                let ts = panels::targets(d);
                let cur = panels::resolve_cursor(cursor.get(), &ts);
                let details = (!wide)
                    .then(|| d.summary.as_ref().map(|s| panels::facts(s, d.definition.as_ref())))
                    .flatten();
                panels::output(d, ts.get(cur), open, details.as_deref())
            });
            let t = abstracttui::app::current_theme().tokens;
            // The inked lines ride the document (they scroll with it), so
            // a short terminal still shows the text under them.
            let text_w = if wide { inner_w - panels::RUNS_PANEL_W - 1 } else { inner_w }
                - panels::PANEL_CHROME
                - 1; // the Scroll's bar column
            let mut content = Element::new().style(LayoutStyle::column().gap(1));
            if !out.meta.is_empty() {
                content = content.child(panels::lines_view(&out.meta, text_w).0);
            }
            content = content.child(panels::markdown(out.doc));
            let body = Scroll::new(content.build())
                .offset_y(out_off)
                .extent_signal(out_ext)
                .viewport_size_signal(out_vp)
                .scrollbar_auto_hide(true)
                .layout(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)))
                .element(scx, &t)
                .build();
            let layout = LayoutStyle::column().grow(1.0).basis(Dimension::Cells(0));
            let layout = if grow { layout } else { layout.min_h(out_min_h) };
            if wide {
                panels::panel(out.title, layout, vec![body]).build()
            } else {
                panels::section(out.title, layout, vec![body]).build()
            }
        };
        // The runs table: its header row, then the engine List of rows.
        let runs_panel = move |scx: Scope, layout: LayoutStyle| -> View {
            let (title, items, cur) = store.automations.with(|v| {
                let d = v.detail.clone().unwrap_or_default();
                let ts = panels::targets(&d);
                let cur = panels::resolve_cursor(cursor.get(), &ts);
                (panels::runs_title(&d), panels::run_items(&d, &ts, cur), cur)
            });
            let labels: Vec<String> = items
                .iter()
                .map(|r| r.lines.iter().map(|l| l.plain()).collect::<Vec<_>>().join(" "))
                .collect();
            let t = abstracttui::app::current_theme().tokens;
            let selection = scx.signal(cur);
            let list = List::new(labels)
                .rich_items(items)
                .selection(selection)
                .offset_y(list_off)
                .scroll_to(list_to)
                .on_select(move |i| {
                    confirm_archive.set(false);
                    cursor.set(i);
                    out_off.set(0);
                })
                .layout(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)))
                .element(scx, &t)
                .build();
            let children = vec![panels::runs_header_view(), list];
            if wide {
                panels::table_panel(title, layout, children)
            } else {
                panels::section(title, layout, children)
            }
                .on(abstracttui::ui::Phase::Bubble, runs_wheel)
                .build()
        };
        Element::new()
            .style(LayoutStyle::column().padding(Edges::all(1)))
            .focusable()
            .autofocus()
            .on(abstracttui::ui::Phase::Capture, nav)
            .on(abstracttui::ui::Phase::Bubble, wheel)
            .shortcut(KeyChord::plain(Key::Escape), move |_| {
                if confirm_archive.get_untracked() {
                    confirm_archive.set(false);
                } else {
                    back()
                }
            })
            .shortcut(key('l'), move |_| toggle_artifacts())
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
            .shortcut(key('o'), move |_| as_chat())
            .shortcut(key('w'), move |_| folder())
            // Ctrl+P reads the selected run's reply aloud (again = stop),
            // the conversation's read-aloud path on this automation's run.
            .shortcut(KeyChord::new(Mods::CTRL, Key::Char('p')), {
                let ctx = ctx2.clone();
                move |_| {
                    if crate::ui::voice_view::stop_speaking(store) {
                        return;
                    }
                    let picked = store.automations.with_untracked(|v| {
                        v.detail.as_ref().map(|d| read_aloud_target(d, at().as_ref()))
                    });
                    match picked {
                        Some(Ok((run_id, text))) => {
                            store
                                .automations
                                .update(|v| v.notice = "Reading the reply aloud — Ctrl+P stops.".into());
                            crate::ui::voice_view::speak_text_for(store, &ctx, run_id, text)
                        }
                        Some(Err(why)) => store.automations.update(|v| v.notice = why),
                        None => {}
                    }
                }
            })
            .shortcut(key('r'), {
                let ctx = ctx2.clone();
                move |_| refresh(store, &ctx)
            })
            // The title: the automation's name, wrapped (never cut).
            .child(dyn_view(LayoutStyle::default().shrink(0.0), move || {
                let title = store.automations.with(|v| {
                    v.detail
                        .as_ref()
                        .and_then(|d| d.summary.as_ref())
                        .map(|s| format!("Automations / {}", s.title))
                        .unwrap_or_else(|| "Automations / Loading automation…".into())
                });
                panels::lines_view(&[vec![(title, panels::Ink::Title)]], inner_w).0
            }))
            // The header panel (and, wide, the Details beside it).
            .child(dyn_view(LayoutStyle::default().shrink(0.0), move || {
                let now = auto::now_unix();
                let (header, facts) = store.automations.with(|v| {
                    match v.detail.as_ref().and_then(|d| d.summary.as_ref().map(|s| (d, s))) {
                        Some((d, s)) => (
                            panels::header_lines(s, v.busy, v.pending_on(&s.id), now),
                            panels::facts(s, d.definition.as_ref()),
                        ),
                        None => (vec![vec![("Loading automation…".to_string(), panels::Ink::Muted)]], Vec::new()),
                    }
                });
                if !wide {
                    // A phone-width terminal: the lines without a frame (two
                    // rows back for the runs and the output).
                    return panels::lines_view(&header, inner_w).0;
                }
                // Each panel's rows wrap at the width they are drawn at; both
                // panels take the taller one's height (aligned borders).
                let (head_w, details_w) = panels::top_widths(inner_w);
                let (lines, head_rows) = panels::lines_view(&header, head_w - panels::PANEL_CHROME);
                let (rows_view, fact_rows) = panels::facts_view(&facts, details_w - panels::PANEL_CHROME);
                let h = head_rows.max(fact_rows) + 2;
                let head = panels::panel(
                    "Schedule".into(),
                    LayoutStyle::column().w(head_w).h(h).shrink(0.0),
                    vec![lines],
                )
                .build();
                let details = panels::panel(
                    "Details".into(),
                    LayoutStyle::column().w(details_w).h(h).shrink(0.0),
                    vec![rows_view],
                )
                .build();
                Element::new()
                    .style(LayoutStyle::row().gap(1))
                    .child(head)
                    .child(details)
                    .build()
            }))
            // The Archive question, errors, notices.
            .child(dyn_view(LayoutStyle::default().shrink(0.0), move || {
                let mut lines: Vec<panels::Line> = Vec::new();
                store.automations.with(|v| {
                    if confirm_archive.get() {
                        if let Some(s) = v.detail.as_ref().and_then(|d| d.summary.as_ref()) {
                            lines.push(vec![(
                                format!("{}  y Archive · n Keep it", archive_question(&s.title)),
                                panels::Ink::Tone(abstracttui::widgets::Tone::Accent),
                            )]);
                        }
                    }
                    if let Some(e) = v.detail.as_ref().map(|d| d.error.clone()).filter(|e| !e.is_empty()) {
                        lines.push(vec![(format!("could not read it: {e}"), panels::Ink::Tone(abstracttui::widgets::Tone::Error))]);
                    }
                    for l in status_lines(v) {
                        lines.push(vec![(l, panels::Ink::Muted)]);
                    }
                });
                panels::lines_view(&lines, inner_w).0
            }))
            // The runs and the selected row's panel.
            // At least: the runs panel's rows and four for the output.
            .child(dyn_view_scoped(
                LayoutStyle::default()
                    .grow(1.0)
                    .basis(Dimension::Cells(0))
                    .min_h(if wide { 10 } else { runs_max_h + out_min_h }),
                move |scx| {
                    if wide {
                        Element::new()
                            .style(LayoutStyle::row().gap(1).grow(1.0))
                            .child(runs_panel(scx, LayoutStyle::column().w(panels::RUNS_PANEL_W).shrink(0.0)))
                            .child(output_panel(scx, true))
                            .build()
                    } else {
                        let rows = store.automations.with(|v| {
                            v.detail.as_ref().map(|d| panels::targets(d).len()).unwrap_or(0)
                        }) as i32;
                        Element::new()
                            .style(LayoutStyle::column().grow(1.0))
                            .child(runs_panel(scx, LayoutStyle::column().h((rows + 2).clamp(3, runs_max_h)).shrink(0.0)))
                            .child(output_panel(scx, false))
                            .build()
                    }
                },
            ))
            // The key hints, then the kit's Run now line: on a short
            // terminal they give up their rows first (the runs and the
            // output keep theirs).
            .child(shrinkable(panels::hints_view(&[panels::AUTOMATION_HINTS, panels::SELECTION_HINTS], inner_w), 1))
            .child(shrinkable(panels::lines_view(&[vec![(auto::run_now_key_line(), panels::Ink::Faint)]], inner_w), 0))
            .build()
    });
}

/// A fixed block of `rows` lines that may give up rows down to `keep`
/// (clipped at its bottom) when the screen is too short for everything.
fn shrinkable((view, rows): (View, i32), keep: i32) -> View {
    Element::new()
        .style(
            LayoutStyle::column()
                .basis(Dimension::Cells(rows))
                .shrink(1.0)
                .min_h(keep.min(rows))
                .clip(),
        )
        .child(view)
        .build()
}

/// Rows a wheel notch moves (3, the engine's step), signed; `None` for any
/// other event.
pub(crate) fn wheel_delta(ev: &abstracttui::ui::UiEvent) -> Option<i32> {
    let abstracttui::ui::UiEvent::Mouse(m) = ev else {
        return None;
    };
    match m.kind {
        abstracttui::ui::MouseKind::ScrollUp => Some(-3),
        abstracttui::ui::MouseKind::ScrollDown => Some(3),
        _ => None,
    }
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

/// `/automation [task]` — the kit's schedule dialog (`crate::ui::schedule_view`).
pub fn open_schedule(cx: Scope, store: Store, ctx: &UiCtx, seed: Option<String>) {
    crate::ui::schedule_view::open_schedule(cx, store, ctx, seed)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::transcript_view::ChatEntry;

    fn occ(index: u64, status: &str, task: &str, answer: &str) -> auto::Occurrence {
        auto::Occurrence {
            run_id: format!("run-{index}"),
            index,
            attempts: 1,
            fired_at: String::new(),
            status: status.into(),
            trigger_summary: String::new(),
            user_turn: task.into(),
            answer: answer.into(),
            notify: None,
            failure: None,
            artifacts: Vec::new(),
            waits: Vec::new(),
        }
    }

    /// ADR 0048: the gateway's order, the paused ones folded under `Paused · N`
    /// (Enter opens them as the same cards); a gateway without `section` gets
    /// every row as today.
    #[test]
    fn paused_automations_fold_under_their_own_line_in_the_served_order() {
        let page: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/automations/list.json"))
                .unwrap();
        let mut items = auto::parse_list_page(&page).unwrap().items;
        assert!(
            items.len() >= 3,
            "the fixture lists at least three automations"
        );
        let older = items.clone();
        for (i, s) in items.iter_mut().enumerate() {
            s.section = Some(if i == 0 { "paused" } else { "active" }.into());
        }
        let ids = |targets: &[ListTarget]| -> Vec<String> {
            targets
                .iter()
                .map(|t| match t {
                    ListTarget::Automation(id) => id.clone(),
                    ListTarget::PausedLine => "<paused>".into(),
                    ListTarget::ArchivedLine => "<archived>".into(),
                    ListTarget::Archived(id) => format!("archived:{id}"),
                })
                .collect()
        };
        let live: Vec<String> = items
            .iter()
            .skip(1)
            .filter(|s| s.status != "archived")
            .map(|s| s.id.clone())
            .collect();
        let mut v = auto::View {
            list: Some(Ok(items.clone())),
            ..Default::default()
        };
        let (cards, targets) = list_cards(&v, 0, None);
        let mut want = live.clone();
        want.push("<paused>".into());
        assert_eq!(
            ids(&targets),
            want,
            "active ones in the served order, then the folded line"
        );
        assert!(cards
            .iter()
            .any(|c| c.lines.iter().any(|l| l.text == "Paused · 1")));
        // The gateway's figure when it serves one (ADR 0048).
        let served = auto::View {
            list: Some(Ok(items.clone())),
            paused_count: Some(7),
            ..Default::default()
        };
        assert!(list_cards(&served, 0, None)
            .0
            .iter()
            .any(|c| c.lines.iter().any(|l| l.text == "Paused · 7")));
        v.paused_open = true;
        let (_, targets) = list_cards(&v, 0, None);
        want.push(items[0].id.clone());
        assert_eq!(
            ids(&targets),
            want,
            "opened: the paused one, as a card, after the line"
        );
        let v = auto::View {
            list: Some(Ok(older.clone())),
            ..Default::default()
        };
        let (_, targets) = list_cards(&v, 0, None);
        let today: Vec<String> = older
            .iter()
            .filter(|s| s.status != "archived")
            .map(|s| s.id.clone())
            .collect();
        assert_eq!(
            ids(&targets),
            today,
            "an older gateway: every row as served, no Paused line"
        );
    }

    #[test]
    fn the_runs_read_as_a_conversation_oldest_first() {
        let occs = vec![
            occ(3, "running", "check", ""),
            occ(1, "completed", "check", "all quiet"),
            occ(2, "completed", "check", "two new"),
        ];
        let entries = occurrences_as_chat(&occs);
        let seps: Vec<&str> = entries
            .iter()
            .filter_map(|e| match e {
                ChatEntry::Separator { key, .. } => Some(key.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            seps,
            vec!["r1", "r2", "r3"],
            "one separator per run, oldest first"
        );
        match &entries[0] {
            ChatEntry::Separator { label, .. } => assert!(label.starts_with("#1 · completed")),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            entries[1],
            ChatEntry::Item(Box::new(crate::transcript::Item::User {
                text: "check".into()
            }))
        );
        assert_eq!(
            entries[2],
            ChatEntry::Item(Box::new(crate::transcript::Item::Assistant {
                text: "all quiet".into(),
                final_answer: true,
                author: None,
            }))
        );
        // A run still going has its task and no reply yet.
        assert_eq!(entries.len(), 3 * 2 + 2);
    }
}
