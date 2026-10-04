//! `/sessions` — the conversations board, at parity with the Code WUI's
//! Conversations list (R7.3).
//!
//! Each conversation is a card: its title (the opening prompt, else
//! `Conversation <8 chars>`) with its live state at the right while it
//! wants something, then `Oct 2 · 2 turns · 7 tools`. `a` archives the
//! selected one after an inline confirmation (the web's sentence), through
//! `POST /sessions/{id}/archive` — nothing is deleted; archiving the open
//! conversation opens the next one (else the previous, else a new one).
//! The quiet `Archived · N` line at the end (N = the gateway's
//! `archived_sessions`) opens the archived conversations inline, each with
//! Unarchive.
//!
//! Existence comes from the GATEWAY (the `/runs` listing, fetched at the
//! gesture and on `r`, never polled); this client's remembered labels only
//! fill a missing title, and a remembered session the gateway did not list
//! says what that absence means (proven / outside this listing / unknown).

use abstracttui::prelude::*;

use crate::conversations as conv;
use crate::gateway::rail::RailCmd;
use crate::runner::Cmd;
use crate::store::{SessionIndex, Store};
use crate::ui::cards::{draw_cards, hint_bar, note_lines, Card, CardLine, Ink};
use crate::ui::modals::{
    merge_session_rows, modal_size, title_row, SessionPick, SESSION_LIST_LIMIT,
};
use crate::ui::UiCtx;

const SPINNER: [char; 8] = ['⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];

/// What the cursor is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardTarget {
    Session(String),
    ArchivedLine,
    Archived(String),
}

/// What it MEANS that the gateway's listing did not contain a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Absence {
    Proven,
    OutsideListing,
    Unknown,
}

fn absence(index: &SessionIndex) -> Absence {
    match index {
        SessionIndex::Loaded {
            truncated: false, ..
        } => Absence::Proven,
        SessionIndex::Loaded { .. } => Absence::OutsideListing,
        _ => Absence::Unknown,
    }
}

/// The board's sessions in display order (live ones first, then newest).
pub fn board_picks(
    index: &SessionIndex,
    local: &[crate::config::SessionEntry],
) -> Vec<SessionPick> {
    match index {
        SessionIndex::Loaded { rows, .. } => merge_session_rows(Some(rows), local),
        _ => merge_session_rows(None, local),
    }
}

fn tools_of(index: &SessionIndex, id: &str) -> Option<u64> {
    match index {
        SessionIndex::Loaded { rows, .. } => rows.iter().find(|r| r.id == id).and_then(|r| r.tools),
        _ => None,
    }
}

/// The provenance line: which facts came from where.
pub fn board_hint(index: &SessionIndex) -> String {
    match index {
        SessionIndex::Unfetched | SessionIndex::Loading => {
            "asking the gateway which sessions exist…".into()
        }
        SessionIndex::Failed(msg) => format!("gateway did not answer — local rows only · {msg}"),
        SessionIndex::Loaded {
            rows,
            truncated,
            labeled,
            ..
        } => {
            let more = if *truncated {
                " · partial: +older runs, counts are floors"
            } else {
                " · complete listing"
            };
            // NAME the prompt bound: only the top `labeled` rows were
            // fetched a prompt (one request each).
            let unlabeled = rows.len().saturating_sub(*labeled);
            let cut = if unlabeled > 0 {
                format!(" · prompts: top {labeled}, {unlabeled} unfetched")
            } else {
                String::new()
            };
            format!("{} sessions on the gateway{more}{cut}", rows.len())
        }
    }
}

/// Everything the board draws (pure, test-pinned). `confirm` = the session
/// whose Archive question shows; `frame` animates the waiting surface.
#[allow(clippy::too_many_arguments)]
pub fn board_cards(
    index: &SessionIndex,
    archived: &SessionIndex,
    archived_open: bool,
    local: &[crate::config::SessionEntry],
    current: &str,
    confirm: Option<&str>,
    frame: usize,
    offset_secs: i64,
) -> (Vec<Card>, Vec<BoardTarget>) {
    let mut cards = Vec::new();
    let mut targets = Vec::new();
    if matches!(index, SessionIndex::Unfetched | SessionIndex::Loading) {
        let spin = SPINNER[frame % SPINNER.len()];
        cards.push(Card::note(format!(
            "{spin}  asking the gateway which sessions exist…"
        )));
        return (cards, targets);
    }
    let gone = absence(index);
    let floor = matches!(
        index,
        SessionIndex::Loaded {
            truncated: true,
            ..
        }
    );
    let picks = board_picks(index, local);
    for p in &picks {
        let marker = if p.id == current { "● " } else { "" };
        let right = match (p.state, gone) {
            (Some(s), _) if s.is_live() => s.label().to_string(),
            (Some(_), _) => String::new(),
            (None, Absence::Proven) => "not on the gateway".into(),
            (None, Absence::OutsideListing) => "outside this listing".into(),
            (None, Absence::Unknown) => "state unknown".into(),
        };
        let mut lines = vec![CardLine::new(
            format!("{marker}{}", conv::card_title(&p.label, &p.id)),
            Ink::Title,
        )
        .right(right)];
        let meta = if p.from_gateway {
            conv::meta_line(&p.when, p.turns, tools_of(index, &p.id), floor, offset_secs)
        } else {
            // A remembered session: its day is this client's memory.
            format!(
                "{} · remembered here",
                conv::day_label(&p.when, offset_secs)
                    .unwrap_or_else(|| "Saved conversation".into())
            )
        };
        lines.push(CardLine::new(meta, Ink::Faint).indent(2));
        if confirm == Some(p.id.as_str()) {
            lines.push(CardLine::new(conv::ARCHIVE_QUESTION, Ink::Accent).indent(2));
            lines.push(CardLine::new("y Archive · n Cancel", Ink::Accent).indent(2));
        }
        cards.push(Card::new(lines));
        targets.push(BoardTarget::Session(p.id.clone()));
    }
    if picks.is_empty() {
        cards.push(Card::note(match index {
            SessionIndex::Loaded { .. } => {
                "Your conversations will live here. Pick up where you left off, on any device."
            }
            _ => "Nothing remembered here, and the gateway did not answer.",
        }));
    }
    let n = match index {
        SessionIndex::Loaded { archived, .. } => *archived,
        _ => 0,
    };
    if n > 0 {
        cards.push(Card::new(vec![CardLine::new(
            format!("Archived · {n}"),
            Ink::Faint,
        )]));
        targets.push(BoardTarget::ArchivedLine);
        if archived_open {
            match archived {
                SessionIndex::Unfetched | SessionIndex::Loading => {
                    cards.push(Card::note("Loading archived conversations…"))
                }
                SessionIndex::Failed(e) => cards.push(Card::fixed(vec![CardLine::new(
                    format!("Archived conversations unavailable: {e}"),
                    Ink::Error,
                )
                .indent(2)])),
                SessionIndex::Loaded { rows, .. } => {
                    for r in rows {
                        let label = r.prompt.clone().unwrap_or_else(|| {
                            local
                                .iter()
                                .find(|e| e.id == r.id)
                                .map(|e| e.label.clone())
                                .unwrap_or_default()
                        });
                        cards.push(Card::new(vec![
                            CardLine::new(conv::card_title(&label, &r.id), Ink::Text)
                                .indent(2)
                                .right("Unarchive"),
                            CardLine::new(
                                conv::meta_line(&r.last_at, r.turns, r.tools, false, offset_secs),
                                Ink::Faint,
                            )
                            .indent(4),
                        ]));
                        targets.push(BoardTarget::Archived(r.id.clone()));
                    }
                }
            }
        }
    }
    (cards, targets)
}

/// The board's key hints.
pub const BOARD_HINTS: &[(&str, &str)] = &[
    ("↑↓", ""),
    ("Enter", "continues"),
    ("a", "Archive"),
    ("u", "Unarchive"),
    ("n", "New conversation"),
    ("r", "Refresh"),
    ("Esc", "closes"),
];

fn load(ctx: &UiCtx, store: Store) {
    if matches!(store.session_index.get_untracked(), SessionIndex::Loading) {
        return; // one listing in flight is enough
    }
    ctx.send(Cmd::LoadSessions {
        limit: SESSION_LIST_LIMIT,
    });
    ctx.send(Cmd::Rail(RailCmd::LoadArchived));
}

/// Archive (or unarchive) one conversation through the gateway.
pub(crate) fn set_archived(store: Store, ctx: &UiCtx, id: &str, archive: bool, ids: &[String]) {
    if store.rail.with_untracked(|r| r.archive_busy) {
        return;
    }
    let next = (archive && id == store.session_id.get_untracked())
        .then(|| conv::next_after_archive(ids, id))
        .flatten();
    store.rail.update(|r| {
        r.archive_busy = true;
        r.board_error.clear();
    });
    ctx.send(Cmd::Rail(RailCmd::SetArchived {
        session_id: id.to_string(),
        archive,
        next,
    }));
}

/// `/sessions` — open the board.
pub fn open_sessions(cx: Scope, store: Store, ctx: &UiCtx) {
    open_board(cx, store, ctx, None);
}

/// `/archive` — the board with the Archive question asked for the open
/// conversation (the web header's ⋯ → Archive).
pub fn open_archive_current(cx: Scope, store: Store, ctx: &UiCtx) {
    let sid = store.session_id.get_untracked();
    open_board(cx, store, ctx, Some(sid));
}

fn open_board(cx: Scope, store: Store, ctx: &UiCtx, ask: Option<String>) {
    store.rail.update(|r| r.board_error.clear());
    load(ctx, store);
    let ctx2 = ctx.clone();
    let size = modal_size(120, 40);
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let cursor = mcx.signal(0usize);
        let confirm = mcx.signal(ask.clone());
        let frame = mcx.signal(0u64);
        // The waiting glyph ticks only while the listing is in flight and
        // only while this board is open (the zero-wakeup idle rule).
        {
            let handle: std::rc::Rc<
                std::cell::RefCell<Option<abstracttui::reactive::IntervalHandle>>,
            > = Default::default();
            mcx.effect(move || {
                let pending = matches!(
                    store.session_index.get(),
                    SessionIndex::Unfetched | SessionIndex::Loading
                );
                let mut slot = handle.borrow_mut();
                match (pending, slot.is_some()) {
                    (true, false) => {
                        *slot = Some(abstracttui::reactive::interval(
                            mcx,
                            std::time::Duration::from_millis(120),
                            move || frame.update(|f| *f = f.wrapping_add(1)),
                        ))
                    }
                    (false, true) => {
                        if let Some(h) = slot.take() {
                            h.cancel();
                        }
                    }
                    _ => {}
                }
            });
        }
        let local = std::rc::Rc::new(ctx2.prefs.borrow().recent_sessions.clone());
        let compute = {
            let local = local.clone();
            move |tracked: bool, confirm_id: Option<String>| {
                let index = if tracked {
                    store.session_index.get()
                } else {
                    store.session_index.get_untracked()
                };
                let (archived, open) = if tracked {
                    store.rail.with(|r| (r.archived.clone(), r.archived_open))
                } else {
                    store
                        .rail
                        .with_untracked(|r| (r.archived.clone(), r.archived_open))
                };
                let now = crate::automations::now_unix();
                board_cards(
                    &index,
                    &archived,
                    open,
                    &local,
                    &store.session_id.get_untracked(),
                    confirm_id.as_deref(),
                    if tracked { frame.get() } else { 0 } as usize,
                    conv::local_offset_secs(now),
                )
            }
        };
        let targets = {
            let compute = compute.clone();
            move || compute(false, None).1
        };
        let target = {
            let targets = targets.clone();
            move || targets().get(cursor.get_untracked()).cloned()
        };
        let session_ids = {
            let targets = targets.clone();
            move || -> Vec<String> {
                targets()
                    .into_iter()
                    .filter_map(|t| match t {
                        BoardTarget::Session(id) => Some(id),
                        _ => None,
                    })
                    .collect()
            }
        };
        let move_cursor = {
            let targets = targets.clone();
            move |delta: i64| {
                confirm.set(None);
                let n = targets().len();
                if n > 0 {
                    cursor.update(|c| *c = (*c as i64 + delta).clamp(0, n as i64 - 1) as usize);
                }
            }
        };
        let enter = {
            let ctx = ctx2.clone();
            let target = target.clone();
            let session_ids = session_ids.clone();
            move || {
                // Nothing is selectable while the board is waiting.
                if matches!(
                    store.session_index.get_untracked(),
                    SessionIndex::Unfetched | SessionIndex::Loading
                ) {
                    return;
                }
                match target() {
                    Some(BoardTarget::Session(id)) => {
                        crate::ui::switch_session(store, &ctx, &id);
                        ctx.close_modal();
                    }
                    Some(BoardTarget::ArchivedLine) => {
                        store.rail.update(|r| r.archived_open = !r.archived_open)
                    }
                    Some(BoardTarget::Archived(id)) => {
                        set_archived(store, &ctx, &id, false, &session_ids())
                    }
                    None => {}
                }
            }
        };
        let archive = {
            let target = target.clone();
            let local = local.clone();
            move || {
                if let Some(BoardTarget::Session(id)) = target() {
                    let local_only = board_picks(&store.session_index.get_untracked(), &local)
                        .iter()
                        .any(|p| p.id == id && !p.from_gateway);
                    if local_only {
                        store.rail.update(|r| {
                            r.board_error =
                                "Not archived: the gateway does not list this conversation.".into()
                        });
                        return;
                    }
                    confirm.set(Some(id));
                }
            }
        };
        let confirm_yes = {
            let ctx = ctx2.clone();
            let session_ids = session_ids.clone();
            move || {
                if let Some(id) = confirm.get_untracked() {
                    confirm.set(None);
                    set_archived(store, &ctx, &id, true, &session_ids());
                }
            }
        };
        let unarchive = {
            let ctx = ctx2.clone();
            let target = target.clone();
            let session_ids = session_ids.clone();
            move || {
                if let Some(BoardTarget::Archived(id)) = target() {
                    set_archived(store, &ctx, &id, false, &session_ids());
                }
            }
        };
        let key = |c: char| KeyChord::plain(Key::Char(c));
        Element::new()
            .style(LayoutStyle::column().padding(Edges::all(1)))
            .focusable()
            .autofocus()
            .shortcut(KeyChord::plain(Key::Escape), {
                let ctx = ctx2.clone();
                move |_| {
                    if confirm.get_untracked().is_some() {
                        confirm.set(None)
                    } else {
                        ctx.close_modal()
                    }
                }
            })
            .shortcut(KeyChord::plain(Key::Up), {
                let m = move_cursor.clone();
                move |_| m(-1)
            })
            .shortcut(KeyChord::plain(Key::Down), move |_| move_cursor(1))
            .shortcut(KeyChord::plain(Key::Enter), move |_| enter())
            .shortcut(key('a'), move |_| archive())
            .shortcut(key('y'), move |_| confirm_yes())
            .shortcut(key('u'), move |_| unarchive())
            .shortcut(key('n'), {
                let ctx = ctx2.clone();
                move |_| {
                    if confirm.get_untracked().is_some() {
                        confirm.set(None);
                    } else {
                        ctx.close_modal();
                        crate::ui::new_session(store, &ctx);
                    }
                }
            })
            .shortcut(key('r'), {
                let ctx = ctx2.clone();
                move |_| load(&ctx, store)
            })
            .child(title_row(&t, "Conversations".into()))
            .child(dyn_view(LayoutStyle::column().shrink(0.0), move || {
                let t2 = abstracttui::app::current_theme().tokens;
                let err = store.rail.with(|r| r.board_error.clone());
                note_lines(&t2, &[board_hint(&store.session_index.get()), err], 8)
            }))
            .child(dyn_view(
                LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),
                move || {
                    let (cards, targets) = compute(true, confirm.get());
                    let cur = cursor.get().min(targets.len().saturating_sub(1));
                    draw_cards(cards, cur)
                },
            ))
            .child(hint_bar(&t, BOARD_HINTS, 8))
            .build()
    });
}
