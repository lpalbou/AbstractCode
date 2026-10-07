//! The kit WorkspaceChooser, in the terminal (R14.4 = the Code web's
//! R11.6 / R13.2 surfaces, same words):
//!
//! - the rail's Workspace panel bound to the conversation = the SESSION
//!   level (this conversation's workspaces, stored by the gateway on the
//!   session) + "My default workspaces" (the ACCOUNT level, a screen);
//! - the rail's Workspace panel bound to an automation = the RUN level
//!   (its definition's `input_data.workspace`, each change one revision);
//! - the new-automation dialog's visible "Workspaces" step = the RUN level
//!   (the value rides the create body's `target.input_data.workspace`).
//!
//! Layout, top to bottom, as the kit draws it: "Gateway: <gateway_summary>"
//! (verbatim) · the follow switch ("Use my default" / "Follow the gateway
//! policy") · the posture · "Allowed workspaces" rows · "Refused
//! workspaces" rows · "Everything else" (posture b) · "Add a workspace
//! path" · the private-workspace note · the effective line (verbatim).
//! Each change is ONE request with the full body; a refusal shows the
//! gateway's sentence + "Not saved." under the control and nothing changes.
//! No policy logic here: modes above a cap come from the gateway's caps and
//! say why with the kit's sentence.

use std::rc::Rc;

use abstracttui::prelude::*;

use crate::gateway::workspaces::{self as lane, RunCommit, WsCmd};
use crate::runner::Cmd;
use crate::store::Store;
use crate::ui::cards::{draw_cards, hint_bar, Card, CardLine, Ink};
use crate::ui::modals::{modal_size, open_picker, title_row, Picker};
use crate::ui::UiCtx;
use crate::workspaces::{self as ws, Level, Mode, Posture, RunValue, State, WsData};

/// Whose workspaces a chooser shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Host {
    /// This conversation (session level).
    Session(String),
    /// "My default workspaces" (account level).
    Account,
    /// The new-automation dialog (run level; the value is `WsData.draft`).
    NewAutomation,
    /// An existing automation (run level; the value is its definition's).
    Automation(String),
}

/// What one chooser control does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsAct {
    Follow,
    Posture,
    Mode(String),
    EverythingElse,
    Add,
    /// Opens "My default workspaces" (session level only).
    MyDefault,
    /// The effective line (reachable so it scrolls into view; Enter does nothing).
    Summary,
}

/// One chooser as the screen reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct HostView {
    pub level: Level,
    pub scope: String,
    pub state: Option<State>,
    pub load_error: Option<String>,
    pub unavailable: Option<String>,
    /// The run level's current value (`None` = "Use my default").
    pub value: Option<RunValue>,
    /// The request key whose answer the chooser needs (`None` = loaded).
    pub needs: Option<String>,
}

/// The automation definition's input as the rail reads it (`None` while it loads).
pub fn automation_input(store: Store, id: &str, tracked: bool) -> Option<(serde_json::Value, u64)> {
    let read = |v: &crate::automations::View| {
        v.detail
            .as_ref()
            .filter(|d| d.id == id)
            .and_then(|d| d.definition.as_ref())
            .map(|def| {
                (
                    def.target
                        .get("input_data")
                        .cloned()
                        .unwrap_or(serde_json::json!({})),
                    def.revision,
                )
            })
    };
    if tracked {
        store.automations.with(read)
    } else {
        store.automations.with_untracked(read)
    }
}

/// Pure: the chooser for `host` from the store's facts.
pub fn host_view(
    host: &Host,
    data: &WsData,
    connected: bool,
    automation: Option<&serde_json::Value>,
) -> HostView {
    let unavailable = (!connected).then(|| ws::DISCONNECTED.to_string());
    let run = |value: Option<RunValue>| {
        let key = ws::run_value_json(value.as_ref()).to_string();
        let (state, load_error, needs) = match data.run_for(&key) {
            Some(Ok(e)) => (Some(ws::run_state(value.as_ref(), e)), None, None),
            Some(Err(e)) => (None, Some(e.clone()), None),
            None => (None, None, Some(format!("run:{key}"))),
        };
        HostView {
            level: Level::Run,
            scope: lane::RUN_SCOPE.to_string(),
            state,
            load_error,
            unavailable: unavailable.clone(),
            value,
            needs,
        }
    };
    match host {
        Host::Session(id) => {
            let (state, load_error, needs) = match &data.session {
                Some((sid, Ok(s))) if sid == id => (Some(s.clone()), None, None),
                Some((sid, Err(e))) if sid == id => (None, Some(e.clone()), None),
                _ => (None, None, Some(format!("session:{id}"))),
            };
            HostView {
                level: Level::Session,
                scope: lane::session_scope(id),
                state,
                load_error,
                unavailable: unavailable.clone(),
                value: None,
                needs,
            }
        }
        Host::Account => {
            let (state, load_error, needs) = match &data.account {
                Some(Ok(s)) => (Some(s.clone()), None, None),
                Some(Err(e)) => (None, Some(e.clone()), None),
                None => (None, None, Some("account".to_string())),
            };
            HostView {
                level: Level::Account,
                scope: lane::ACCOUNT_SCOPE.to_string(),
                state,
                load_error,
                unavailable,
                value: None,
                needs,
            }
        }
        Host::NewAutomation => run(data.draft.clone()),
        Host::Automation(_) => match automation {
            Some(input) => run(ws::run_value_from(input)),
            None => HostView {
                level: Level::Run,
                scope: lane::RUN_SCOPE.to_string(),
                state: None,
                load_error: None,
                unavailable,
                value: None,
                needs: None,
            },
        },
    }
}

/// Ask the gateway for what `hv` still needs (once per key in flight).
pub fn ensure_loaded(store: Store, ctx: &UiCtx, host: &Host, hv: &HostView) {
    let Some(key) = hv.needs.clone() else { return };
    if hv.unavailable.is_some() {
        return;
    }
    let in_flight = store
        .workspaces
        .with_untracked(|w| w.loading.contains(&key));
    if in_flight {
        return;
    }
    store.workspaces.update(|w| w.loading.push(key.clone()));
    let cmd = match host {
        Host::Session(id) => WsCmd::LoadSession {
            session_id: id.clone(),
        },
        Host::Account => WsCmd::LoadAccount,
        Host::NewAutomation | Host::Automation(_) => WsCmd::DryRun {
            value: hv.value.clone(),
        },
    };
    ctx.send(Cmd::Workspaces(cmd));
}

fn faint(text: impl Into<String>) -> CardLine {
    CardLine::new(text, Ink::Faint).indent(2)
}

fn status_line(data: &WsData, scope: &str, key: &str) -> Option<CardLine> {
    data.status_of(scope, key).map(|s| {
        CardLine::new(
            s.text.clone(),
            if s.error { Ink::Error } else { Ink::Faint },
        )
        .indent(2)
    })
}

/// The segmented control of a row as one quiet line: `(•)` current,
/// `( )` offered, `(-)` above the gateway's cap.
fn options_line(current: Mode, modes: &[Mode], unavailable: &dyn Fn(Mode) -> bool) -> String {
    modes
        .iter()
        .map(|m| {
            let mark = if *m == current {
                "(•)"
            } else if unavailable(*m) {
                "(-)"
            } else {
                "( )"
            };
            format!("{mark} {}", m.label())
        })
        .collect::<Vec<_>>()
        .join("  ")
}

/// Pure: the chooser's cards and what each selectable card does.
pub fn chooser_cards(hv: &HostView, data: &WsData) -> (Vec<Card>, Vec<WsAct>) {
    let mut cards: Vec<Card> = Vec::new();
    let mut acts: Vec<WsAct> = Vec::new();
    // The kit chooser titles itself "Workspaces" at every level (the account
    // screen's own title is "My default workspaces").
    cards.push(Card::fixed(vec![
        CardLine::new(ws::TITLE, Ink::Title),
        CardLine::new(hv.level.help(), Ink::Faint),
    ]));
    if let Some(err) = hv
        .load_error
        .as_ref()
        .or(hv.unavailable.as_ref().filter(|_| hv.state.is_none()))
    {
        cards.push(Card::fixed(vec![CardLine::new(err.clone(), Ink::Error)]));
        return (cards, acts);
    }
    let Some(state) = hv.state.as_ref() else {
        cards.push(Card::note(ws::LOADING));
        return (cards, acts);
    };
    let view = ws::view(hv.level, state);
    let blocked = hv.unavailable.is_some();
    let busy = data.busy.as_ref().is_some_and(|(s, _)| *s == hv.scope);
    let scope = hv.scope.as_str();
    let push = |cards: &mut Vec<Card>,
                acts: &mut Vec<WsAct>,
                mut lines: Vec<CardLine>,
                key: &str,
                act: Option<WsAct>| {
        if let Some(st) = status_line(data, scope, key) {
            lines.push(st);
        }
        match act {
            Some(a) if !blocked => {
                cards.push(Card::new(lines));
                acts.push(a);
            }
            _ => cards.push(Card::fixed(lines)),
        }
    };
    cards.push(Card::fixed(vec![CardLine::new(
        view.gateway_line.clone(),
        Ink::Faint,
    )]));
    if let Some(reason) = &hv.unavailable {
        cards.push(Card::fixed(vec![CardLine::new(reason.clone(), Ink::Error)]));
    }
    if view.locked {
        cards.push(Card::note(ws::LOCKED));
    }
    // The follow switch.
    let on = view.following;
    let switch = CardLine::new(
        format!(
            "{} {}",
            if on { "[x]" } else { "[ ]" },
            hv.level.follow_label()
        ),
        if view.locked || blocked {
            Ink::Faint
        } else if on {
            Ink::On
        } else {
            Ink::Text
        },
    )
    .right(
        if busy && data.busy.as_ref().is_some_and(|(_, k)| k == "follow") {
            "…"
        } else {
            ""
        },
    );
    push(
        &mut cards,
        &mut acts,
        vec![switch, faint(hv.level.follow_help())],
        "follow",
        (!view.locked).then_some(WsAct::Follow),
    );
    // The posture.
    let posture_lines = vec![
        CardLine::new(ws::POSTURE_LABEL, Ink::Text).right(view.posture.label()),
        faint(view.posture.help()),
    ];
    let editable = !view.following && !view.locked;
    push(
        &mut cards,
        &mut acts,
        posture_lines,
        "posture",
        editable.then_some(WsAct::Posture),
    );
    let row_card = |cards: &mut Vec<Card>, acts: &mut Vec<WsAct>, row: &ws::Row| {
        let mut lines = vec![CardLine::new(row.path.clone(), Ink::Text).right(row.mode.label())];
        if row.editable {
            lines.push(faint(options_line(row.mode, &Mode::ALL, &|m| {
                !row.allowed.contains(&m)
            })));
            let mut reasons: Vec<&str> = row.reasons.iter().map(|(_, r)| *r).collect();
            reasons.dedup();
            for r in reasons {
                lines.push(faint(r));
            }
        }
        if let Some(st) = status_line(data, scope, &row.path) {
            lines.push(st);
        }
        if row.editable && !blocked {
            cards.push(Card::new(lines));
            acts.push(WsAct::Mode(row.path.clone()));
        } else {
            cards.push(Card::fixed(lines));
        }
    };
    let allowed = view.allowed();
    if !allowed.is_empty() {
        cards.push(Card::fixed(vec![CardLine::new(
            ws::ALLOWED_TITLE,
            Ink::Title,
        )]));
        for row in &allowed {
            row_card(&mut cards, &mut acts, row);
        }
    }
    let refused = view.refused();
    if !refused.is_empty() {
        cards.push(Card::fixed(vec![CardLine::new(
            ws::DENIED_TITLE,
            Ink::Title,
        )]));
        for row in &refused {
            row_card(&mut cards, &mut acts, row);
        }
    }
    if let Some((mode, editable)) = view.everything_else {
        let mut lines = vec![CardLine::new(ws::EVERYTHING_ELSE, Ink::Text).right(mode.label())];
        if editable {
            lines.push(faint(options_line(mode, &[Mode::Rw, Mode::Ro], &|_| false)));
        }
        push(
            &mut cards,
            &mut acts,
            lines,
            "everything-else",
            editable.then_some(WsAct::EverythingElse),
        );
    }
    if view.can_add {
        push(
            &mut cards,
            &mut acts,
            vec![CardLine::new(ws::ADD_PLACEHOLDER, Ink::Accent).right(ws::ADD)],
            "add",
            Some(WsAct::Add),
        );
    }
    if view.posture == Posture::AllowedOnly && allowed.is_empty() {
        cards.push(Card::note(ws::EMPTY_ALLOWED));
    }
    if matches!(hv.level, Level::Session | Level::Run) {
        cards.push(Card::note(ws::PRIVATE_NOTE));
    }
    // The effective line, verbatim; the cursor can land on it so it always
    // scrolls into view (nothing happens on Enter).
    cards.push(Card::new(vec![CardLine::new(
        view.summary.clone(),
        Ink::Accent,
    )]));
    acts.push(WsAct::Summary);
    if hv.level == Level::Session {
        cards.push(Card::new(vec![CardLine::new(
            ws::MY_DEFAULT_WORKSPACES,
            Ink::Text,
        )
        .right("Enter opens")]));
        acts.push(WsAct::MyDefault);
    }
    (cards, acts)
}

/// Send one change of `host` (the full body).
fn submit(
    store: Store,
    ctx: &UiCtx,
    host: &Host,
    hv: &HostView,
    payload: serde_json::Value,
    key: &str,
) {
    store.workspaces.update(|w| {
        w.busy = Some((hv.scope.clone(), key.to_string()));
        if w.status.as_ref().is_some_and(|s| s.scope == hv.scope) {
            w.status = None;
        }
    });
    let key = key.to_string();
    let cmd = match host {
        Host::Session(id) => WsCmd::SaveSession {
            session_id: id.clone(),
            payload,
            key,
        },
        Host::Account => WsCmd::SaveAccount { payload, key },
        Host::NewAutomation => WsCmd::RunChange {
            value: ws::payload_run_value(&payload),
            key,
            commit: RunCommit::Draft,
        },
        Host::Automation(id) => {
            let Some((changes, expected_revision)) = store
                .automations
                .with_untracked(|v| {
                    v.detail
                        .as_ref()
                        .filter(|d| &d.id == id)
                        .and_then(|d| d.definition.clone())
                })
                .map(|def| {
                    (
                        crate::rail::workspace_changes(
                            &def.target,
                            ws::payload_run_value(&payload).as_ref(),
                        ),
                        def.revision,
                    )
                })
            else {
                store.workspaces.update(|w| w.busy = None);
                return;
            };
            let Some(changes) = changes else {
                store.workspaces.update(|w| w.busy = None);
                return;
            };
            store
                .rail
                .update(|r| r.save = crate::rail::SaveState::Saving);
            WsCmd::RunChange {
                value: ws::payload_run_value(&payload),
                key,
                commit: RunCommit::Revision {
                    id: id.clone(),
                    command_id: crate::config::mint_session_id(),
                    expected_revision,
                    changes,
                },
            }
        }
    };
    ctx.send(Cmd::Workspaces(cmd));
}

/// The current chooser of `host` (untracked: inside a handler).
pub fn current(store: Store, host: &Host) -> HostView {
    let connected = matches!(
        store.conn.get_untracked(),
        crate::store::Conn::Ok | crate::store::Conn::Unknown
    );
    let input = match host {
        Host::Automation(id) => automation_input(store, id, false).map(|(i, _)| i),
        _ => None,
    };
    store
        .workspaces
        .with_untracked(|w| host_view(host, w, connected, input.as_ref()))
}

/// Run one chooser control; `back` reopens the screen it came from.
pub fn run_ws_act(
    cx: Scope,
    store: Store,
    ctx: &UiCtx,
    host: &Host,
    act: WsAct,
    back: Rc<dyn Fn()>,
) {
    if act == WsAct::Summary {
        return;
    }
    if act == WsAct::MyDefault {
        open_account_screen(cx, store, ctx, back);
        return;
    }
    let hv = current(store, host);
    let Some(state) = hv.state.clone() else {
        return;
    };
    if hv.unavailable.is_some() || store.workspaces.with_untracked(|w| w.busy.is_some()) {
        return;
    }
    let view = ws::view(hv.level, &state);
    let pick = |title: String, labels: Vec<String>, start: usize, choose: Rc<dyn Fn(usize)>| {
        let back2 = back.clone();
        let back3 = back.clone();
        let rows = labels.len() as i32;
        open_picker(
            cx,
            ctx,
            Picker {
                title: format!("{title} — ↑↓ · Enter chooses · Esc back"),
                labels,
                live: None,
                start,
                size: modal_size(110, (rows + 6).min(30)),
                hint: None,
                live_hint: None,
                keys: Vec::new(),
                on_mount: None,
                on_selection: None,
                on_choose: Box::new(move |ix| {
                    choose(ix);
                    back2();
                }),
                on_cancel: Some(Box::new(move || back3())),
            },
        );
    };
    match act {
        WsAct::MyDefault | WsAct::Summary => {}
        WsAct::Follow => {
            if view.locked {
                return;
            }
            submit(
                store,
                ctx,
                host,
                &hv,
                ws::follow_payload(&state, !view.following),
                "follow",
            );
        }
        WsAct::Posture => {
            if view.following || view.locked {
                return;
            }
            let labels = Posture::ALL
                .iter()
                .map(|p| format!("{} — {}", p.label(), p.help()))
                .collect();
            let start = Posture::ALL
                .iter()
                .position(|p| *p == view.posture)
                .unwrap_or(0);
            let (ctx2, host2, hv2, policy) =
                (ctx.clone(), host.clone(), hv.clone(), state.policy.clone());
            pick(
                ws::POSTURE_LABEL.to_string(),
                labels,
                start,
                Rc::new(move |ix| {
                    let p = Posture::ALL[ix.min(1)];
                    if p != policy.posture {
                        submit(
                            store,
                            &ctx2,
                            &host2,
                            &hv2,
                            ws::posture_payload(&policy, p),
                            "posture",
                        );
                    }
                }),
            );
        }
        WsAct::EverythingElse => {
            let Some((mode, true)) = view.everything_else else {
                return;
            };
            let modes = [Mode::Rw, Mode::Ro];
            let start = modes.iter().position(|m| *m == mode).unwrap_or(0);
            let (ctx2, host2, hv2, policy) =
                (ctx.clone(), host.clone(), hv.clone(), state.policy.clone());
            pick(
                format!("{} {}", ws::ACCESS_LABEL, ws::EVERYTHING_ELSE),
                modes.iter().map(|m| m.label().to_string()).collect(),
                start,
                Rc::new(move |ix| {
                    let m = modes[ix.min(1)];
                    if m != policy.default_mode {
                        submit(
                            store,
                            &ctx2,
                            &host2,
                            &hv2,
                            ws::default_mode_payload(&policy, m),
                            "everything-else",
                        );
                    }
                }),
            );
        }
        WsAct::Mode(path) => {
            let Some(row) = view
                .rows
                .iter()
                .find(|r| r.path == path && r.editable)
                .cloned()
            else {
                return;
            };
            let mut labels: Vec<String> = Mode::ALL
                .iter()
                .map(|m| match row.reason(*m) {
                    Some(why) => format!("{} — {why}", m.label()),
                    None => m.label().to_string(),
                })
                .collect();
            labels.push(format!("{} {}", ws::REMOVE, row.path));
            let start = Mode::ALL.iter().position(|m| *m == row.mode).unwrap_or(0);
            let (ctx2, host2, hv2, policy) =
                (ctx.clone(), host.clone(), hv.clone(), state.policy.clone());
            pick(
                format!("{} {}", ws::ACCESS_LABEL, row.path),
                labels,
                start,
                Rc::new(move |ix| {
                    if ix >= Mode::ALL.len() {
                        submit(
                            store,
                            &ctx2,
                            &host2,
                            &hv2,
                            ws::remove_payload(&policy, &row.path),
                            &row.path,
                        );
                        return;
                    }
                    let m = Mode::ALL[ix];
                    if let Some(why) = row.reason(m) {
                        // Above the gateway's cap: never sent (the kit's disabled option).
                        store.notify(why);
                        return;
                    }
                    if m != row.mode {
                        submit(
                            store,
                            &ctx2,
                            &host2,
                            &hv2,
                            ws::mode_payload(&policy, &row.path, m),
                            &row.path,
                        );
                    }
                }),
            );
        }
        WsAct::Add => {
            if !view.can_add {
                return;
            }
            let (ctx2, host2, hv2, policy) =
                (ctx.clone(), host.clone(), hv.clone(), state.policy.clone());
            let back2 = back.clone();
            crate::ui::automations_view::open_text(
                cx,
                ctx,
                ws::ADD_PLACEHOLDER.to_string(),
                vec![hv.level.help().to_string()],
                String::new(),
                Rc::new(move |path: String| {
                    if !path.trim().is_empty() {
                        submit(
                            store,
                            &ctx2,
                            &host2,
                            &hv2,
                            ws::add_payload(&policy, &path),
                            "add",
                        );
                    }
                    back2();
                }),
                back,
            );
        }
    }
}

/// The key hints of a chooser screen.
pub const SCREEN_HINTS: &[(&str, &str)] = &[("↑↓", ""), ("Enter", "change"), ("Esc", "back")];

/// A full chooser screen for `host` (the account level, the new-automation
/// step). `next` = an extra last card (label, action) — the dialog's
/// "Continue". Esc runs `back`.
pub fn open_screen(
    cx: Scope,
    store: Store,
    ctx: &UiCtx,
    host: Host,
    title: String,
    next: Option<(String, Rc<dyn Fn()>)>,
    back: Rc<dyn Fn()>,
) {
    let ctx2 = ctx.clone();
    let size = modal_size(120, 40);
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        // With a "Continue" card the cursor starts on it (Enter accepts the
        // shown workspaces); `usize::MAX` = the last card, clamped on use.
        let cursor = mcx.signal(if next.is_some() { usize::MAX } else { 0usize });
        // Load what the chooser needs (and re-load after a change elsewhere).
        {
            let ctx = ctx2.clone();
            let host = host.clone();
            mcx.effect(move || {
                let connected = matches!(
                    store.conn.get(),
                    crate::store::Conn::Ok | crate::store::Conn::Unknown
                );
                let hv = store
                    .workspaces
                    .with(|w| host_view(&host, w, connected, None));
                ensure_loaded(store, &ctx, &host, &hv);
            });
        }
        let cards_of = {
            let host = host.clone();
            let next = next.clone();
            Rc::new(move |tracked: bool| -> (Vec<Card>, Vec<Option<WsAct>>) {
                let connected = matches!(
                    if tracked {
                        store.conn.get()
                    } else {
                        store.conn.get_untracked()
                    },
                    crate::store::Conn::Ok | crate::store::Conn::Unknown
                );
                let read = |w: &WsData| {
                    let hv = host_view(&host, w, connected, None);
                    chooser_cards(&hv, w)
                };
                let (mut cards, acts) = if tracked {
                    store.workspaces.with(read)
                } else {
                    store.workspaces.with_untracked(read)
                };
                let mut acts: Vec<Option<WsAct>> = acts.into_iter().map(Some).collect();
                if let Some((label, _)) = &next {
                    cards.push(Card::new(vec![
                        CardLine::new(label.clone(), Ink::Accent).right("Enter")
                    ]));
                    acts.push(None);
                }
                (cards, acts)
            })
        };
        let activate = {
            let ctx = ctx2.clone();
            let host = host.clone();
            let cards_of = cards_of.clone();
            let next = next.clone();
            let back = back.clone();
            Rc::new(move || {
                let (_, acts) = cards_of(false);
                let at = cursor.get_untracked().min(acts.len().saturating_sub(1));
                match acts.get(at).cloned() {
                    Some(Some(act)) => {
                        let reopen: Rc<dyn Fn()> = {
                            let ctx = ctx.clone();
                            let host = host.clone();
                            let title = title.clone();
                            let next = next.clone();
                            let back = back.clone();
                            Rc::new(move || {
                                open_screen(
                                    cx,
                                    store,
                                    &ctx,
                                    host.clone(),
                                    title.clone(),
                                    next.clone(),
                                    back.clone(),
                                )
                            })
                        };
                        run_ws_act(cx, store, &ctx, &host, act, reopen);
                    }
                    Some(None) => {
                        if let Some((_, go)) = &next {
                            go();
                        }
                    }
                    None => {}
                }
            })
        };
        let move_cursor = {
            let cards_of = cards_of.clone();
            move |delta: i64| {
                let n = cards_of(false).1.len();
                if n > 0 {
                    cursor.update(|c| {
                        let at = (*c).min(n - 1) as i64;
                        *c = (at + delta).clamp(0, n as i64 - 1) as usize
                    });
                }
            }
        };
        let title2 = title_for(&host);
        Element::new()
            .style(LayoutStyle::column().padding(Edges::all(1)))
            .focusable()
            .autofocus()
            .shortcut(KeyChord::plain(Key::Escape), {
                let back = back.clone();
                move |_| back()
            })
            .shortcut(KeyChord::plain(Key::Up), {
                let m = move_cursor.clone();
                move |_| m(-1)
            })
            .shortcut(KeyChord::plain(Key::Down), move |_| move_cursor(1))
            .shortcut(KeyChord::plain(Key::Enter), {
                let a = activate.clone();
                move |_| a()
            })
            .shortcut(KeyChord::plain(Key::Char(' ')), {
                let a = activate.clone();
                move |_| a()
            })
            .child(dyn_view(LayoutStyle::line(1).shrink(0.0), move || {
                let t2 = abstracttui::app::current_theme().tokens;
                title_row(&t2, title2.clone())
            }))
            .child(dyn_view(
                LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),
                move || {
                    let (cards, acts) = cards_of(true);
                    let cur = cursor.get().min(acts.len().saturating_sub(1));
                    draw_cards(cards, cur)
                },
            ))
            .child(hint_bar(&t, SCREEN_HINTS, 8))
            .build()
    });
}

fn title_for(host: &Host) -> String {
    match host {
        Host::Account => ws::MY_DEFAULT_WORKSPACES.to_string(),
        Host::NewAutomation => format!("new automation — 5/6 {}", ws::TITLE),
        _ => ws::TITLE.to_string(),
    }
}

/// "My default workspaces" (the account level); Esc runs `back`.
pub fn open_account_screen(cx: Scope, store: Store, ctx: &UiCtx, back: Rc<dyn Fn()>) {
    open_screen(
        cx,
        store,
        ctx,
        Host::Account,
        ws::MY_DEFAULT_WORKSPACES.to_string(),
        None,
        back,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const SESSION_DEFAULT: &str =
        include_str!("../../tests/fixtures/workspaces/session_get_default.json");
    const SESSION_CONFIGURED: &str =
        include_str!("../../tests/fixtures/workspaces/session_get_configured.json");
    const DRY_DEFAULT: &str = include_str!("../../tests/fixtures/workspaces/dryrun_default.json");

    fn v(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    fn texts(cards: &[Card]) -> Vec<String> {
        cards
            .iter()
            .flat_map(|c| c.lines.iter().map(|l| format!("{} | {}", l.text, l.right)))
            .collect()
    }

    fn data_with_session(answer: &str) -> WsData {
        WsData {
            session: Some(("s1".into(), Ok(ws::as_state(&v(answer)).unwrap()))),
            ..WsData::default()
        }
    }

    #[test]
    fn the_session_panel_reads_top_to_bottom_like_the_kit() {
        let data = data_with_session(SESSION_CONFIGURED);
        let hv = host_view(&Host::Session("s1".into()), &data, true, None);
        let (cards, acts) = chooser_cards(&hv, &data);
        let t = texts(&cards);
        let at = |needle: &str| {
            t.iter()
                .position(|l| l.starts_with(needle))
                .unwrap_or_else(|| panic!("{needle} in {t:#?}"))
        };
        assert!(at("Workspaces |") < at("Gateway: "));
        assert!(at("Gateway: ") < at("[ ] Use my default"));
        assert!(at("[ ] Use my default") < at("Workspaces agents may use"));
        assert!(at("Workspaces agents may use") < at("Allowed workspaces"));
        assert!(at("Allowed workspaces") < at("Add a workspace path"));
        assert!(at("Add a workspace path") < at("The private workspace of each run"));
        assert!(
            at("The private workspace of each run")
                < at("Deny everything, allow listed workspaces · ")
        );
        assert!(t
            .iter()
            .any(|l| l == "The gateway allows this workspace read-only | "));
        assert!(t
            .iter()
            .any(|l| l.starts_with("(-) Read & write  (•) Read-only  ( ) Refused")));
        assert_eq!(acts.first(), Some(&WsAct::Follow));
        assert_eq!(acts.last(), Some(&WsAct::MyDefault));
        assert!(acts.contains(&WsAct::Posture));
        assert!(acts.contains(&WsAct::Add));
        assert_eq!(
            acts.iter().filter(|a| matches!(a, WsAct::Mode(_))).count(),
            2
        );
    }

    #[test]
    fn following_the_default_shows_rows_without_controls() {
        let data = data_with_session(SESSION_DEFAULT);
        let hv = host_view(&Host::Session("s1".into()), &data, true, None);
        let (cards, acts) = chooser_cards(&hv, &data);
        let t = texts(&cards);
        assert!(t.iter().any(|l| l == "[x] Use my default | "));
        assert!(t.iter().any(|l| l.starts_with("Refused workspaces")));
        assert!(t
            .iter()
            .any(|l| l.starts_with("Everything else | Read & write")));
        assert_eq!(acts, vec![WsAct::Follow, WsAct::Summary, WsAct::MyDefault]);
    }

    #[test]
    fn a_refusal_shows_under_its_control_and_disconnected_blocks_changes() {
        let mut data = data_with_session(SESSION_CONFIGURED);
        let path = "/Users/ada/home/Pictures".to_string();
        data.status = Some(ws::Status {
            scope: "session:s1".into(),
            key: path.clone(),
            text: ws::refusal(
                "The gateway allows this workspace read-only: /Users/ada/home/Pictures.",
            ),
            error: true,
        });
        let hv = host_view(&Host::Session("s1".into()), &data, true, None);
        let (cards, _) = chooser_cards(&hv, &data);
        let card = cards.iter().find(|c| c.lines[0].text == path).unwrap();
        assert_eq!(
            card.lines.last().unwrap().text,
            "The gateway allows this workspace read-only: /Users/ada/home/Pictures. Not saved."
        );
        assert_eq!(card.lines.last().unwrap().ink, Ink::Error);
        let off = host_view(&Host::Session("s1".into()), &data, false, None);
        let (cards, acts) = chooser_cards(&off, &data);
        assert!(texts(&cards)
            .iter()
            .any(|l| l.starts_with(ws::DISCONNECTED)));
        assert_eq!(acts, vec![WsAct::Summary, WsAct::MyDefault]);
    }

    #[test]
    fn hosts_ask_for_what_they_miss() {
        let data = WsData::default();
        assert_eq!(
            host_view(&Host::Session("s1".into()), &data, true, None)
                .needs
                .as_deref(),
            Some("session:s1")
        );
        assert_eq!(
            host_view(&Host::Account, &data, true, None)
                .needs
                .as_deref(),
            Some("account")
        );
        assert_eq!(
            host_view(&Host::NewAutomation, &data, true, None)
                .needs
                .as_deref(),
            Some("run:null")
        );
        let loaded = WsData {
            runs: vec![(
                "null".into(),
                Ok(ws::as_effective(&v(DRY_DEFAULT)).unwrap()),
            )],
            ..WsData::default()
        };
        let hv = host_view(&Host::NewAutomation, &loaded, true, None);
        assert!(hv.needs.is_none());
        let (cards, acts) = chooser_cards(&hv, &loaded);
        assert!(texts(&cards).iter().any(|l| l == "[x] Use my default | "));
        assert_eq!(
            acts,
            vec![WsAct::Follow, WsAct::Summary],
            "the run level has no account link"
        );
        let auto = host_view(
            &Host::Automation("a1".into()),
            &loaded,
            true,
            Some(
                &serde_json::json!({"workspace": {"posture": "allowed_only", "default_mode": "rw", "folders": []}}),
            ),
        );
        assert!(auto.needs.as_deref().unwrap().starts_with("run:{"));
    }
}
