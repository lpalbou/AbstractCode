//! Entity conversation actions driven from the UI thread: opening a
//! conversation from an `@` chip, turn sends + held drafts, focus
//! switching, /end, /task, the `@entity` mention send, and the poller
//! view sync. All signal writes happen here (UI thread); HTTP rides the
//! runner's entity commands.

use abstracttui::prelude::*;

use crate::convo::{self, ConvoStatus, EntityConvo, Focus};
use crate::runner::Cmd;
use crate::store::Store;
use crate::ui::UiCtx;

/// Open a conversation with `name` (or focus the existing one). Closed and
/// refused conversations reopen fresh — "the next @name opens fresh".
pub fn open_or_focus(store: Store, ctx: &UiCtx, name: &str) {
    let name = name.to_lowercase();
    let cached_state = store.entities.with_untracked(|es| {
        es.iter()
            .find(|e| e.slug == name)
            .map(|e| e.state.clone())
            .unwrap_or_default()
    });
    let mut needs_open = false;
    store.convos.update(|cs| match convo::find(cs, &name) {
        Some(ix) => {
            if matches!(cs[ix].status, ConvoStatus::Closed | ConvoStatus::Refused) {
                convo::fold_reopen(&mut cs[ix], &cached_state);
                needs_open = true;
            }
        }
        None => {
            cs.push(EntityConvo::opening(&name, &cached_state));
            needs_open = true;
        }
    });
    if needs_open {
        ctx.send(Cmd::EntityOpen { name: name.clone() });
    }
    store.focus.set(Focus::Entity(name));
}

/// Send a turn into an entity conversation — or HOLD the draft while the
/// entity is opening/mid-turn (the ruled v1 between-turns steering).
pub fn send_or_hold(store: Store, ctx: &UiCtx, name: &str, text: &str) {
    let name = name.to_lowercase();
    let mut send: Option<(String, u64)> = None;
    let mut send_flow: Option<(String, u64)> = None;
    let mut notice: Option<String> = None;
    store.convos.update(|cs| {
        let Some(ix) = convo::find(cs, &name) else {
            notice = Some(format!("no conversation with {name} — @{name} opens one"));
            return;
        };
        match cs[ix].status {
            ConvoStatus::Ready | ConvoStatus::Parked => {
                let epoch = convo::fold_send_turn(&mut cs[ix], text);
                if cs[ix].brain == convo::Brain::Flow {
                    send_flow = Some((cs[ix].session_id.clone(), epoch));
                } else {
                    send = Some((cs[ix].run_id.clone(), epoch));
                }
            }
            ConvoStatus::Opening | ConvoStatus::TurnRunning => {
                let verb = if cs[ix].status == ConvoStatus::Opening {
                    "opens"
                } else {
                    "finishes this turn"
                };
                convo::hold_draft(&mut cs[ix], text);
                notice = Some(format!("held — sends when {name} {verb}"));
            }
            ConvoStatus::Closed | ConvoStatus::Refused => {
                notice = Some(if cs[ix].brain == convo::Brain::Flow {
                    format!("this conversation ended — /brain {name} opens a fresh one")
                } else {
                    format!("this visit is closed — @{name} opens a fresh one")
                });
            }
        }
    });
    if let Some((run_id, epoch)) = send {
        ctx.send(Cmd::EntityTurn {
            name,
            run_id,
            epoch,
            text: text.to_string(),
        });
    } else if let Some((session_id, epoch)) = send_flow {
        ctx.send(Cmd::EntityFlowTurn {
            name,
            session_id,
            epoch,
            text: text.to_string(),
        });
    }
    if let Some(n) = notice {
        store.notify(n);
    }
}

/// `/brain <name>` — open (or focus) a FLOW-BRAIN conversation:
/// summon-per-prompt of the `entity-chat` flow, no server-side visit.
/// One conversation per entity name stands (whatever its brain) — the
/// reference implementation's P0 chimera lesson: never replace a live
/// thread's transport under it. An existing convo focuses with a notice
/// naming its brain; `/end` first to switch.
pub fn open_flow_convo(store: Store, name: &str) {
    // FIRST WORD ONLY (adversary P1-1): "/brain castor hello" must not
    // mint a junk conversation literally named "castor hello".
    let name = name.split_whitespace().next().unwrap_or("").to_lowercase();
    if name.is_empty() {
        store.notify("usage: /brain <name>");
        return;
    }
    // Roster check, mention-path parity: a LOADED roster that lacks the
    // name refuses (typo protection); an EMPTY roster (not yet fetched)
    // proceeds — the first summon errors honestly if the name is wrong.
    let known = store
        .entities
        .with_untracked(|es| es.is_empty() || es.iter().any(|e| e.slug == name));
    if !known {
        store.notify(format!(
            "no entity named {name} on this gateway — /entities lists the roster"
        ));
        return;
    }
    let mut opened = false;
    let mut notice: Option<String> = None;
    let cached_state = store.entities.with_untracked(|es| {
        es.iter()
            .find(|e| e.slug == name)
            .map(|e| e.state.clone())
            .unwrap_or_default()
    });
    store.convos.update(|cs| {
        if let Some(ix) = convo::find(cs, &name) {
            match cs[ix].status {
                ConvoStatus::Closed | ConvoStatus::Refused => {
                    // A finished thread is history — replace it with the
                    // fresh flow conversation (same rule as @name reopen).
                    // EPOCH INHERITANCE is load-bearing: a fresh record
                    // starting at 0 would let a STALE in-flight thread
                    // from the old conversation (send epoch 1 → /end →
                    // replace → new send epoch 1) match `guard_flow` and
                    // fold the WRONG entity's-reply into the new thread.
                    // Carrying the old epoch forward (already bumped past
                    // every in-flight post by fold_flow_end) keeps every
                    // stale post guarded out.
                    let sid = flow_session_id();
                    let mut fresh =
                        crate::convo::EntityConvo::flow_opening(&name, &cached_state, &sid);
                    fresh.turn_epoch = cs[ix].turn_epoch;
                    cs[ix] = fresh;
                    opened = true;
                }
                _ => {
                    notice = Some(if cs[ix].brain == convo::Brain::Flow {
                        format!("already conversing with {name} through the flow brain")
                    } else {
                        format!(
                            "a live visit with {name} exists — /end {name} first, then /brain {name}"
                        )
                    });
                }
            }
        } else {
            let sid = flow_session_id();
            cs.push(crate::convo::EntityConvo::flow_opening(
                &name,
                &cached_state,
                &sid,
            ));
            opened = true;
        }
    });
    store.focus.set(crate::convo::Focus::Entity(name.clone()));
    if opened {
        store.notify(format!(
            "flow-brain conversation with {name} — type to send the first summon"
        ));
    } else if let Some(n) = notice {
        store.notify(n);
    }
}

/// One session id per flow conversation (client-minted): every summon of
/// this conversation groups under it, so continuity rides the entity's
/// graph while the view stays session-local.
fn flow_session_id() -> String {
    // Nanos + pid: the id is now part of the staleness GUARD (adversary
    // P0-1 fix made it load-bearing), so an entropy-free mint would be a
    // collision hazard across processes/rapid opens.
    let ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("tui-flow-{ns:x}-{}", std::process::id())
}

/// `/end [name] [reason]` — refused while a turn is in flight (a close
/// during a live turn races the drive loop server-side; no honest outcome).
pub fn end_visit(store: Store, ctx: &UiCtx, name: Option<&str>, reason: &str) {
    let target = match name {
        Some(n) => Some(n.to_lowercase()),
        None => match store.focus.get_untracked() {
            Focus::Entity(n) => Some(n),
            Focus::Agent => None,
        },
    };
    let Some(name) = target else {
        store.notify("usage: /end <name> [reason] (or /end while focused on the visit)");
        return;
    };
    let mut cmd: Option<Cmd> = None;
    let mut notice: Option<String> = None;
    let mut flow_end = false;
    store.convos.update(|cs| {
        let Some(ix) = convo::find(cs, &name) else {
            notice = Some(format!("no conversation with {name}"));
            return;
        };
        // FLOW-BRAIN conversations close LOCALLY: there is no server
        // visit — each summon already completed; the entity's memory of
        // the conversation persists in its graph. A turn in flight is
        // invalidated by the epoch bump inside fold_flow_end (the summon
        // run completes server-side and forms its memory regardless).
        if cs[ix].brain == convo::Brain::Flow {
            if matches!(cs[ix].status, ConvoStatus::Closed | ConvoStatus::Refused) {
                notice = Some(format!("the conversation with {name} is already closed"));
            } else {
                convo::fold_flow_end(&mut cs[ix]);
                flow_end = true;
            }
            return;
        }
        match cs[ix].status {
            ConvoStatus::TurnRunning => {
                notice = Some(format!("turn in flight — /end when {name} parks"));
            }
            ConvoStatus::Closed | ConvoStatus::Refused => {
                notice = Some(format!("the visit with {name} is already closed"));
            }
            ConvoStatus::Opening => {
                notice = Some("still opening — /end once the visit is up".to_string());
            }
            ConvoStatus::Ready | ConvoStatus::Parked => {
                cmd = Some(Cmd::EntityClose {
                    name: name.clone(),
                    run_id: cs[ix].run_id.clone(),
                    epoch: cs[ix].turn_epoch,
                    reason: if reason.is_empty() {
                        "closed from abstractcode".to_string()
                    } else {
                        reason.to_string()
                    },
                });
            }
        }
    });
    if flow_end {
        store.notify(format!(
            "flow-brain conversation with {name} ended (view only)"
        ));
    }
    if let Some(c) = cmd {
        store.notify(format!("closing the visit with {name} — reflection runs"));
        ctx.send(c);
    }
    if let Some(n) = notice {
        store.notify(n);
    }
}

/// `/task <name> <title>` — durable delegation, no visit needed.
pub fn leave_task(store: Store, ctx: &UiCtx, name: &str, title: &str) {
    if name.is_empty() || title.is_empty() {
        store.notify("usage: /task <name> <title>");
        return;
    }
    let name = name.to_lowercase();
    let on_roster = store
        .entities
        .with_untracked(|es| es.iter().any(|e| e.error.is_empty() && e.slug == name));
    if !on_roster {
        store.notify(format!(
            "@{name} is not on the roster — /entities lists them"
        ));
        return;
    }
    ctx.send(Cmd::EntityTask {
        name,
        title: title.to_string(),
    });
}

/// `/focus <name|agent>`. Follow re-arms via the root effect on focus
/// change (per-conversation scroll positions deliberately NOT kept in v1).
pub fn focus_by_word(store: Store, word: &str) {
    let w = word.trim().to_lowercase();
    if w.is_empty() {
        store.notify("usage: /focus <name|agent>");
        return;
    }
    if w == "agent" {
        store.focus.set(Focus::Agent);
        return;
    }
    let exists = store
        .convos
        .with_untracked(|cs| convo::find(cs, &w).is_some());
    if exists {
        store.focus.set(Focus::Entity(w));
    } else {
        store.notify(format!("no conversation with {w} — @{w} opens one"));
    }
}

/// Alt+E: agent → convo 1 → convo 2 → … → agent.
///
/// CYCLE order is the convos vec order, always. The header may PAINT
/// chips in a different order (the focused chip moves to the front when
/// it would otherwise hide behind "+N" — `convo::chip_paint_plan`), but
/// that is presentation only: cycling here never consults the paint plan,
/// so Alt+E lands on the same next conversation whatever the width.
pub fn cycle_focus(store: Store) {
    let names: Vec<String> = store
        .convos
        .with_untracked(|cs| cs.iter().map(|c| c.name.clone()).collect());
    if names.is_empty() {
        store.notify("no entity conversations yet — @name opens one");
        return;
    }
    let next = match store.focus.get_untracked() {
        Focus::Agent => Focus::Entity(names[0].clone()),
        Focus::Entity(cur) => match names.iter().position(|n| *n == cur) {
            Some(ix) if ix + 1 < names.len() => Focus::Entity(names[ix + 1].clone()),
            _ => Focus::Agent,
        },
    };
    store.focus.set(next);
}

/// Focus-switch side effect (created once in root()): every focus change
/// re-arms follow so the pane lands at the new conversation's tail.
pub fn wire_focus_follow(cx: Scope, store: Store, follow: Signal<bool>) {
    let last = std::rc::Rc::new(std::cell::RefCell::new(store.focus.get_untracked()));
    cx.effect(move || {
        let f = store.focus.get();
        let mut prev = last.borrow_mut();
        if *prev != f {
            *prev = f;
            follow.set(true);
        }
    });
}

/// The honesty notice for agent-run commands issued under entity focus
/// (the command still targets the AGENT run — say so).
pub fn agent_command_notice(store: Store, verb: &str) {
    if matches!(store.focus.get_untracked(), Focus::Entity(_)) {
        store.notify(format!(
            "{verb} targets the agent run — entity turns are non-interruptible"
        ));
    }
}

/// True when the focused ENTITY conversation should swallow Esc-Esc with
/// the non-interruptible notice (never a fake cancel).
pub fn escape_in_entity_focus(store: Store) -> bool {
    let Focus::Entity(name) = store.focus.get_untracked() else {
        return false;
    };
    let running = store.convos.with_untracked(|cs| {
        convo::find(cs, &name)
            .map(|ix| cs[ix].status == ConvoStatus::TurnRunning)
            .unwrap_or(false)
    });
    if running {
        store.notify(format!(
            "{name}'s turn is non-interruptible — it completes server-side (Alt+E switches focus)"
        ));
    }
    // Entity focus consumes the cancel arm entirely: there is no agent-run
    // double-Esc semantics to arm while the user is looking at an entity.
    true
}

/// True while any conversation needs the spinner/elapsed ticker.
pub fn any_convo_active(store: Store) -> bool {
    store.convos.with(|cs| {
        cs.iter()
            .any(|c| matches!(c.status, ConvoStatus::Opening | ConvoStatus::TurnRunning))
    })
}

/// Keep the poller's view of open conversations in sync (an effect in
/// root(); the poller thread reads the mutex, never signals).
pub fn wire_poller_view(cx: Scope, store: Store) {
    cx.effect(move || {
        store.convos.with(|cs| {
            crate::gateway::entities::sync_poller_view(cs);
        });
    });
}

/// Composer placeholder while an ENTITY conversation is focused. The
/// AGENT placeholder is phase-swapped and owned by `ui::agent_placeholder`
/// — ONE authority (cycle-3 audit: the `Focus::Agent` arm that used to
/// live here was production-dead — `ui::root` matches Agent first — and
/// its text had drifted from the live Ctrl+J teaching).
pub fn entity_placeholder(name: &str) -> String {
    format!("message {name} — non-interruptible mid-turn; Enter holds during a turn · /help")
}

/// The `@entity` half of a message with chips (2026-11-08), called right
/// after the person's own turn was sent: one `Cmd::Mentions` on the runner
/// channel (so it follows that turn to the gateway), the history hint
/// recorded as told for this session, and one line naming who is asked.
/// No calls: nothing. A gateway that has no mention route: its sentence,
/// and the message stays the person's turn alone.
pub fn send_mentions(
    store: Store,
    ctx: &UiCtx,
    text: &str,
    draft: &str,
    chips: Vec<crate::mention::MentionCall>,
    repeat_turn: bool,
    calls: Vec<crate::mention::MentionCall>,
) {
    if calls.is_empty() {
        return;
    }
    if store.mentions_off.get_untracked() {
        store.notify(format!("{}.", crate::mention::NOT_OFFERED));
        return;
    }
    let session_id = store.session_id.get_untracked();
    store.mention_told.update(|told| {
        for c in &calls {
            let key = (session_id.clone(), c.entity.clone());
            if !told.contains(&key) {
                told.push(key);
            }
        }
    });
    store.notify(format!("asking {}", names_of(&calls)));
    ctx.send(Cmd::Mentions {
        session_id,
        text: text.to_string(),
        draft: draft.to_string(),
        chips,
        repeat_turn,
        repeat_of: None,
        message: None,
        calls,
    });
}

/// The remembered message of a stopped mention chain (`store.mention_retry`)
/// when `draft` is exactly the draft it put back, in this session; taken
/// either way (any other send is a new message and drops it).
pub fn take_mention_retry(store: Store, draft: &str) -> Option<crate::gateway::mentions::Unsent> {
    let held = store.mention_retry.get_untracked()?;
    store.mention_retry.set(None);
    let here = store.session_id.with_untracked(|s| *s == held.session_id);
    (here && held.draft == draft && !held.calls.is_empty()).then_some(held)
}

/// Resend a stopped chain (client review 2026-11-09 B2): ONLY the calls not
/// answered, with the words as first sent, the same `repeat_of` and the same
/// `mention_id` key (a call whose answer was lost gets it back); no
/// turn starts and no steer goes out, and an entity that answered is never
/// asked again.
pub fn resend_mentions(
    store: Store,
    ctx: &UiCtx,
    held: crate::gateway::mentions::Unsent,
    chips: Vec<crate::mention::MentionCall>,
) {
    if store.mentions_off.get_untracked() {
        store.notify(format!("{}.", crate::mention::NOT_OFFERED));
        return;
    }
    store.notify(format!("asking {} again", names_of(&held.calls)));
    ctx.send(Cmd::Mentions {
        session_id: held.session_id,
        text: held.text,
        draft: held.draft,
        chips,
        repeat_turn: false,
        repeat_of: held.repeat_of,
        message: Some(held.message),
        calls: held.calls,
    });
}

/// Under an entity's focus the message went to that entity as typed; the
/// chips in it asked no one else — say so (mentions belong to the agent
/// conversation, whose session records the answers).
pub fn mentions_not_sent_here(store: Store, calls: &[crate::mention::MentionCall]) {
    if calls.is_empty() {
        return;
    }
    store.notify(format!(
        "{} not asked: mentions are sent from the agent conversation (Alt+E switches)",
        names_of(calls)
    ));
}

/// A slash command carried chips: its argument is text, so they asked no
/// one — say so.
pub fn mentions_in_a_command(store: Store, calls: &[crate::mention::MentionCall]) {
    if calls.is_empty() {
        return;
    }
    store.notify(format!(
        "{} not asked: mentions are sent with a message, not with a command",
        names_of(calls)
    ));
}

/// "Castor", "Castor and Pollux", "A, B and C" (each name once, in order).
fn names_of(calls: &[crate::mention::MentionCall]) -> String {
    let mut names: Vec<&str> = Vec::new();
    for c in calls {
        if !names.contains(&c.name.as_str()) {
            names.push(&c.name);
        }
    }
    match names.as_slice() {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}
