//! `/automation` ("New automation"; `/schedule` until 0.9.2) — the kit's
//! AfScheduleDialog in the terminal (R17.2): seven
//! visible steps, the kit's sections in the kit's order and words (no
//! "Advanced"):
//!
//! 1. What — the workflow ("Gateway default" first; the executable
//!    workflows of `GET /bundles?executable_for=abstractcode.agent.v1`) and
//!    the task (a multiline editor: the composer's `TextArea`, Enter and
//!    Ctrl+J insert a newline, a "Continue" button keeps it);
//! 2. When — Repeat (presets, every N minutes/hours/days, a fixed UTC
//!    interval), Daily / Weekly ([x] day toggles) / Monthly (1–31 or last)
//!    at a time of day, Once at… (a wall time in the account's time zone),
//!    or "When an email arrives" (only while `GET /me/email` says the
//!    account can be used; otherwise the kit's "Connect a mailbox first —
//!    open My email"). Every schedule kind's line is the gateway's
//!    (`POST …/schedule-preview` → `first_run_sentence`, + the time-zone
//!    line for the wall-clock kinds); all are written as `schedule@2`;
//! 3. Context — Independent / Growing (+ "Max growing context (tokens)");
//! 4. Tools — "Use workflow default tools", "Select all" / "Unselect all",
//!    each toolset's header with a tri-state box (`[x]` all, `[ ]` none,
//!    `[~]` some) and the `/tools` rows; every tool starts DESELECTED
//!    (operator ruling 2026-10-09) + Run without asking / Ask me. Its own
//!    scrolling list: the wheel scrolls, a click toggles a line, Space
//!    toggles the focused line and the focus stays on it;
//! 5. Workspaces — the kit chooser at the run level;
//! 6. Mailbox — "Email result" and its recipients;
//! 7. Title and limits — then "Create automation".
//!
//! "Create automation" builds the body the Code web builds: the kit's body
//! with `target.input_data` = the workflow's real inputs (its schema
//! defaults + the conversation's provider/model/tools/skills, validated
//! against the schema — `crate::schedule_input`), the tool selection and
//! the workspaces. A refusal (the kit's, the validator's or the gateway's
//! sentence) shows on the last step.
//!
//! Each step is a card list: ↑↓ (PgUp/PgDn, Home/End) move, Enter (or
//! Space) changes the selected row or continues; Esc cancels. A step
//! starts with the cursor on "Continue", so Enter-Enter-… creates with the
//! defaults; a change keeps the cursor on the row changed (End goes back
//! to Continue).

use std::rc::Rc;

use abstracttui::prelude::*;
use serde_json::{Map, Value};

use crate::automation_email::{self as email, EmailStatus};
use crate::automations::{self as auto, CreateForm, When};
use crate::gateway::automations::AutoCmd;
use crate::runner::Cmd;
use crate::schedule_input::{self as si, Conversation};
use crate::store::Store;
use crate::transcript::Item;
use crate::ui::cards::{draw_cards, hint_bar, Card, CardLine, Ink};
use crate::ui::modals::{modal_size, open_picker, title_row, Picker};
use crate::ui::UiCtx;
use crate::workflow_picker as wp;

/// The kit's section names, in order (the step titles).
pub const STEPS: [&str; 7] = [
    "What",
    "When",
    "Context",
    "Tools",
    crate::workspaces::TITLE,
    "Mailbox",
    LIMITS_TITLE,
];
/// The kit dialog's "Title and limits" section, visible (no Advanced).
pub const LIMITS_TITLE: &str = "Title and limits";
/// The kit's "Ask me" radio, verbatim.
pub const ASK_LABEL: &str = "Ask me before each tool call (the run waits for you)";
pub const AUTO_LABEL: &str = "Run without asking";
pub const ASK_HINT: &str = "Each tool call waits for your approval in the automation's timeline.";
pub const DEFAULT_TOOLS_LABEL: &str = "Use workflow default tools";
pub const TOOLS_HINT: &str =
    "Choose which tools this automation can use. An empty selection disables tools. Gateway restrictions always apply.";
pub const INDEPENDENT_LABEL: &str = "Independent — each run starts fresh";
pub const GROWING_LABEL: &str = "Growing — each run sees the previous runs";
pub const NOT_USABLE_SWITCH: &str = "Connect a mailbox first.";

/// The kit's When presets.
pub const WHEN_PRESETS: [(&str, &str, char); 6] = [
    ("every 5 minutes", "5", 'm'),
    ("every 30 minutes", "30", 'm'),
    ("every hour", "1", 'h'),
    ("every 8 hours", "8", 'h'),
    ("every 24 hours", "24", 'h'),
    ("every 7 days", "7", 'd'),
];

/// The dialog's name: the Code web's words for creating an automation
/// (the sidebar's "New automation" button, `web/src/workspace/
/// automations_view.tsx`), verbatim.
pub const DIALOG_TITLE: &str = "New automation";

/// The step title: "New automation — 2/7 When".
pub fn step_title(n: usize) -> String {
    format!("{DIALOG_TITLE} — {n}/{} {}", STEPS.len(), STEPS[n - 1])
}

/// A workflow's (bundle, version, flow) — the key of its input schema.
pub type WorkflowKey = (String, String, String);

/// A workflow picked in the What step (the web's `chosenTarget`).
#[derive(Debug, Clone, PartialEq)]
pub struct Picked {
    pub label: String,
    pub target: Value,
    /// (bundle, version, flow) of the workflow whose inputs it runs, or why
    /// the gateway default cannot be resolved.
    pub schema: Result<WorkflowKey, String>,
}

/// The dialog's state across its steps.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub form: CreateForm,
    /// The conversation's workflow as a target (the web's `props.target`).
    pub conv_target: Option<Value>,
    pub conv_label: String,
    /// The conversation workflow's (bundle, version, flow).
    pub conv_schema: Option<(String, String, String)>,
    pub picked: Option<Picked>,
    /// The conversation's run settings, read when the dialog opened.
    pub conv: Conversation,
    /// The Tools section was initialised (the first visit sets it).
    pub tools_set: bool,
    /// The calendar rule's fields, kept across kind switches (the kit's
    /// `CalendarRuleState`: Weekly → Monthly → Weekly keeps the days).
    pub rule: auto::CalendarRuleState,
    /// The form as the dialog opened (Esc on step 1 asks "Discard?" only
    /// when something was entered since).
    pub opened: Box<CreateForm>,
}

/// Esc on step 1 when something was entered: the question, in place.
pub const DISCARD_QUESTION: &str =
    "Discard this new automation? Esc discards it · any other key keeps editing.";

impl Draft {
    /// Something was entered since the dialog opened (the task, a workflow,
    /// any step's value; the Tools step's own first selection does not count).
    pub fn edited(&self) -> bool {
        let opened = CreateForm {
            tools: self.form.tools.clone(),
            ..(*self.opened).clone()
        };
        self.picked.is_some() || self.form != opened
    }
}

impl Draft {
    /// The target the automation runs and the schema key of its inputs.
    pub fn target_and_schema(&self) -> (Option<Value>, Result<WorkflowKey, String>) {
        match &self.picked {
            Some(p) => (Some(p.target.clone()), p.schema.clone()),
            None => (
                self.conv_target.clone(),
                self.conv_schema
                    .clone()
                    .ok_or_else(|| "Choose a workflow in the Workflow panel first.".to_string()),
            ),
        }
    }

    pub fn workflow_label(&self) -> String {
        match &self.picked {
            Some(p) => p.label.clone(),
            None => self.conv_label.clone(),
        }
    }
}

/// The conversation's run settings an automation inherits — the same
/// sources an agent turn reads (`agent_start_opts`), minus what an
/// automation owns itself (workspace, consent policy, project context).
pub fn conversation_of(store: Store) -> Conversation {
    let deltas = store
        .host_contracts
        .with_untracked(|c| c.as_ref().map(|c| c.deltas));
    Conversation {
        provider: store.provider.get_untracked(),
        model: store.model.get_untracked(),
        reasoning: store.reasoning.get_untracked(),
        speculation: store.speculation.get_untracked(),
        stream: crate::streaming::run_input_value(store.stream_replies.get_untracked(), deltas),
        max_iterations: store.max_iterations.get_untracked() as u64,
        max_tokens: store.context_window.get_untracked(),
        tools: crate::ui::conversation_tools(store),
        skills: store.selected_skills.get_untracked(),
    }
}

/// The conversation workflow's (bundle, version, flow) — the gateway
/// default's resolution when the conversation follows it.
fn conv_schema_key(w: &crate::store::Workflow) -> Option<(String, String, String)> {
    (!w.bundle_id.trim().is_empty() && !w.flow_id.trim().is_empty()).then(|| {
        (
            w.bundle_id.trim().to_string(),
            w.version.trim().to_string(),
            w.flow_id.trim().to_string(),
        )
    })
}

fn send(ctx: &UiCtx, cmd: AutoCmd) {
    let _ = ctx.send(Cmd::Automations(cmd));
}

fn email_status(store: Store) -> Option<EmailStatus> {
    store.automations.with_untracked(|v| v.email.clone())
}

fn usable(store: Store) -> bool {
    EmailStatus::usable(email_status(store).as_ref())
}

/// What this session knows of the gateway's round-16 schedule API.
fn schedule_api(store: Store) -> auto::ScheduleApi {
    store.automations.with_untracked(|v| v.schedule_api)
}

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

/// `/automation [task]` (and its old name `/schedule`).
pub fn open_schedule(cx: Scope, store: Store, ctx: &UiCtx, seed: Option<String>) {
    let workflow = store.workflow.get_untracked();
    let conv_schema = conv_schema_key(&workflow);
    // Fresh each time (the user may have just connected their mailbox).
    send(
        ctx,
        AutoCmd::Prepare {
            schema: conv_schema.clone(),
        },
    );
    let conv_label = if workflow.flow_id.is_empty() && !workflow.gateway_default {
        String::new()
    } else if workflow.gateway_default {
        format!("Gateway default ({})", workflow.versioned_label())
    } else {
        workflow.versioned_label()
    };
    let form = CreateForm {
        prompt: seed.unwrap_or_else(|| last_prompt(store)),
        ..CreateForm::default()
    };
    let draft = Draft {
        opened: Box::new(form.clone()),
        form,
        conv_target: auto::target_for(&workflow),
        conv_label,
        conv_schema,
        picked: None,
        conv: conversation_of(store),
        tools_set: false,
        rule: auto::CalendarRuleState::default(),
    };
    step_what(cx, store, ctx, draft, Vec::new(), None);
}

// ---------------------------------------------------------------------------
// The generic step: a card list with a cursor
// ---------------------------------------------------------------------------

const STEP_HINTS: &[(&str, &str)] = &[
    ("↑↓", ""),
    ("Enter", "change / continue"),
    ("End", "Continue"),
    ("Esc", "back"),
];

type Build<A> = Rc<dyn Fn() -> (Vec<Card>, Vec<A>)>;

/// Where the cursor starts: on `focus` (the row the user just changed —
/// a change reopens the step, and the cursor stays where it was) or, on a
/// step's first visit, on the LAST selectable card (Continue / Create
/// automation), so Enter-Enter-… creates with the defaults.
pub fn start_cursor<A: PartialEq>(acts: &[A], focus: Option<&A>) -> usize {
    focus
        .and_then(|f| acts.iter().position(|a| a == f))
        .unwrap_or(acts.len().saturating_sub(1))
}

/// Open one step: `build` gives the cards and what each selectable card
/// does (re-read on every frame, so live answers render); `act` runs the
/// selected card's action. The cursor starts at [`start_cursor`].
fn step<A: Clone + PartialEq + 'static>(
    cx: Scope,
    ctx: &UiCtx,
    title: String,
    build: Build<A>,
    act: Rc<dyn Fn(A)>,
    focus: Option<A>,
    back: Rc<dyn Fn()>,
) {
    step_keyed(cx, ctx, title, build, act, focus, back, None)
}

/// [`step`] with `on_key`: called on every key but Esc (step 1 clears its
/// "Discard?" question with it).
#[allow(clippy::too_many_arguments)]
fn step_keyed<A: Clone + PartialEq + 'static>(
    cx: Scope,
    ctx: &UiCtx,
    title: String,
    build: Build<A>,
    act: Rc<dyn Fn(A)>,
    focus: Option<A>,
    back: Rc<dyn Fn()>,
    on_key: Option<Rc<dyn Fn()>>,
) {
    let (cards0, acts0) = build();
    let rows: i32 = cards0.iter().map(|c| c.lines.len() as i32 + 1).sum();
    let size = modal_size(100, rows + 8);
    let start = start_cursor(&acts0, focus.as_ref());
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let cursor = mcx.signal(start);
        let activate = {
            let build = build.clone();
            let act = act.clone();
            let on_key = on_key.clone();
            Rc::new(move || {
                if let Some(k) = &on_key {
                    k();
                }
                let (_, acts) = build();
                if let Some(a) = acts.get(cursor.get_untracked()).cloned() {
                    act(a);
                }
            })
        };
        let n_of = {
            let build = build.clone();
            move || build().1.len()
        };
        let move_cursor = move |delta: i64| {
            let n = n_of();
            if n > 0 {
                cursor.update(|c| *c = (*c as i64 + delta).clamp(0, n as i64 - 1) as usize);
            }
        };
        let mv = Rc::new(move |delta: i64| {
            if let Some(k) = &on_key {
                k();
            }
            move_cursor(delta)
        });
        let build_v = build.clone();
        Element::new()
            .style(LayoutStyle::column().padding(Edges::all(1)))
            .focusable()
            .autofocus()
            // Esc = the previous step (step 1: close, asking first when
            // something was entered) — never the whole dialog at once.
            .shortcut(KeyChord::plain(Key::Escape), move |_| back())
            .shortcut(KeyChord::plain(Key::Up), {
                let mv = mv.clone();
                move |_| mv(-1)
            })
            .shortcut(KeyChord::plain(Key::Down), {
                let mv = mv.clone();
                move |_| mv(1)
            })
            .shortcut(KeyChord::plain(Key::PageUp), {
                let mv = mv.clone();
                move |_| mv(-10)
            })
            .shortcut(KeyChord::plain(Key::PageDown), {
                let mv = mv.clone();
                move |_| mv(10)
            })
            .shortcut(KeyChord::plain(Key::Home), {
                let mv = mv.clone();
                move |_| mv(-(i32::MAX as i64))
            })
            .shortcut(KeyChord::plain(Key::End), {
                let mv = mv.clone();
                move |_| mv(i32::MAX as i64)
            })
            .shortcut(KeyChord::plain(Key::Enter), {
                let a = activate.clone();
                move |_| a()
            })
            .shortcut(KeyChord::plain(Key::Char(' ')), {
                let a = activate.clone();
                move |_| a()
            })
            .child(title_row(&t, title.clone()))
            .child(dyn_view(
                LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),
                move || {
                    let (cards, acts) = build_v();
                    draw_cards(cards, cursor.get().min(acts.len().saturating_sub(1)))
                },
            ))
            .child(hint_bar(&t, STEP_HINTS, 8))
            .build()
    });
}

/// A labelled value card ("Title" / its value, or the faint placeholder).
fn value_card(label: &str, value: &str, empty: &str) -> Card {
    Card::new(vec![
        CardLine::new(label, Ink::Text),
        if value.trim().is_empty() {
            CardLine::new(empty.to_string(), Ink::Faint).indent(2)
        } else {
            CardLine::new(value.trim().to_string(), Ink::Text).indent(2)
        },
    ])
}

fn radio(on: bool, label: &str) -> Card {
    Card::new(vec![CardLine::new(
        format!("{}{label}", if on { "(•) " } else { "( ) " }),
        if on { Ink::On } else { Ink::Text },
    )])
}

fn switch(on: bool, label: &str) -> Card {
    Card::new(vec![CardLine::new(
        format!("{}{label}", if on { "[x] " } else { "[ ] " }),
        if on { Ink::On } else { Ink::Text },
    )])
}

fn continue_card(next: usize) -> Card {
    Card::new(vec![CardLine::new(
        format!("Continue — {}", STEPS[next - 1]),
        Ink::Accent,
    )
    .right("Enter")])
}

fn errors_cards(cards: &mut Vec<Card>, errors: &[String]) {
    for e in errors {
        cards.push(Card::fixed(vec![CardLine::new(e.clone(), Ink::Error)]));
    }
}

/// Edit one text value, then reopen the step with the result.
fn edit_text(
    cx: Scope,
    ctx: &UiCtx,
    title: String,
    info: &str,
    initial: String,
    apply: Rc<dyn Fn(String)>,
    back: Rc<dyn Fn()>,
) {
    crate::ui::automations_view::open_text(
        cx,
        ctx,
        title,
        vec![info.to_string()],
        initial,
        apply,
        back,
    );
}

/// The Task editor's line under its title.
pub const TASK_INFO: &str = "What every run is asked to do (sent as the prompt of every run).";
/// The kit's task placeholder.
pub const TASK_PLACEHOLDER: &str =
    "e.g. Check the price of ACME shares and notify me if it moved more than 2%.";
/// The Task editor's keys: Enter and Ctrl+J insert a newline (a task is
/// a multiline text — Enter never leaves a multiline field), Continue
/// keeps it.
pub const TASK_HINTS: &[(&str, &str)] = &[
    ("Enter / Ctrl+J", "newline"),
    ("Home/End", "line start/end"),
    ("Tab", "Continue"),
    ("Esc", "goes back"),
];

/// The Task editor: the main composer's multiline widget (`TextArea`:
/// soft wrap, internal scroll, Home/End per line, Ctrl+J newline) with
/// Enter inserting a newline too, and a visible "Continue" button (Tab
/// reaches it; a click presses it). Esc goes back without keeping.
pub fn open_task_editor(
    cx: Scope,
    ctx: &UiCtx,
    title: String,
    info: &'static str,
    initial: String,
    apply: Rc<dyn Fn(String)>,
    back: Rc<dyn Fn()>,
) {
    let size = modal_size(100, 24);
    let max_rows = (size.h - 9).max(3);
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let state = abstracttui::widgets::TextAreaState::new(mcx);
        state.set_text(initial.clone());
        let keep = {
            let (state, apply) = (state.clone(), apply.clone());
            move || apply(state.text())
        };
        let area = abstracttui::widgets::TextArea::new()
            .state(&state)
            .placeholder(TASK_PLACEHOLDER)
            .placeholder_while_focused(true)
            .submit_policy(abstracttui::widgets::SubmitPolicy::EnterInserts)
            .rows(3.min(max_rows), max_rows)
            .element(mcx, &t)
            .autofocus()
            .build();
        Element::new()
            .style(LayoutStyle::column().gap(1).padding(Edges::all(1)))
            .shortcut(KeyChord::plain(Key::Escape), {
                let back = back.clone();
                move |_| back()
            })
            .child(title_row(&t, title.clone()))
            .child(crate::ui::cards::note_lines(&t, &[info.to_string()], 8))
            .child(
                Element::new()
                    .style(LayoutStyle::column().grow(1.0).basis(Dimension::Cells(0)))
                    .child(area)
                    .build(),
            )
            .child(
                Element::new()
                    .style(LayoutStyle::row().gap(2).shrink(0.0))
                    .child(
                        Button::new(format!("Continue — {}", STEPS[1]))
                            .on_click(keep)
                            .view(mcx),
                    )
                    .build(),
            )
            .child(hint_bar(&t, TASK_HINTS, 8))
            .build()
    });
}

// ---------------------------------------------------------------------------
// 1 · What
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WhatAct {
    Workflow,
    Task,
    Next,
}

/// Pure: the What step's cards.
pub fn what_cards(
    d: &Draft,
    executable: Option<&Result<wp::Executable, String>>,
    errors: &[String],
) -> (Vec<Card>, Vec<WhatAct>) {
    let mut cards = vec![Card::heading("What")];
    let mut acts = Vec::new();
    let label = d.workflow_label();
    let mut lines = vec![CardLine::new("Workflow", Ink::Text)];
    lines.push(if label.is_empty() {
        CardLine::new("Choose what to run.", Ink::Faint).indent(2)
    } else {
        CardLine::new(label, Ink::Text).indent(2)
    });
    if let Some(Err(e)) = executable {
        lines.push(CardLine::new(e.clone(), Ink::Error).indent(2));
    }
    cards.push(Card::new(lines));
    acts.push(WhatAct::Workflow);
    cards.push(value_card(
        "Task",
        &d.form.prompt,
        "e.g. Check the price of ACME shares and notify me if it moved more than 2%.",
    ));
    acts.push(WhatAct::Task);
    errors_cards(&mut cards, errors);
    cards.push(continue_card(2));
    acts.push(WhatAct::Next);
    (cards, acts)
}

fn step_what(
    cx: Scope,
    store: Store,
    ctx: &UiCtx,
    d: Draft,
    errors: Vec<String>,
    focus: Option<WhatAct>,
) {
    // Esc asks "Discard?" (when something was entered); any other key clears
    // the question and the dialog stays; the next Esc asks again.
    let asked = cx.signal(false);
    let back: Rc<dyn Fn()> = {
        let ctx = ctx.clone();
        let d = d.clone();
        Rc::new(move || {
            if asked.get_untracked() || !d.edited() {
                ctx.close_modal();
            } else {
                asked.set(true);
            }
        })
    };
    let on_key: Rc<dyn Fn()> = Rc::new(move || {
        if asked.get_untracked() {
            asked.set(false);
        }
    });
    let d0 = d.clone();
    let build: Build<WhatAct> = Rc::new(move || {
        let exec = store.automations.with(|v| v.executable.clone());
        let mut errors = errors.clone();
        if asked.get() {
            errors.push(DISCARD_QUESTION.to_string());
        }
        what_cards(&d0, exec.as_ref(), &errors)
    });
    let ctx2 = ctx.clone();
    let act = Rc::new(move |a: WhatAct| match a {
        WhatAct::Workflow => pick_workflow(cx, store, &ctx2, d.clone()),
        WhatAct::Task => {
            let (c3, d3) = (ctx2.clone(), d.clone());
            let (c4, d4) = (ctx2.clone(), d.clone());
            open_task_editor(
                cx,
                &ctx2,
                format!("{} · Task", step_title(1)),
                TASK_INFO,
                d.form.prompt.clone(),
                Rc::new(move |v: String| {
                    let mut d = d3.clone();
                    d.form.prompt = v;
                    step_what(cx, store, &c3, d, Vec::new(), Some(WhatAct::Task));
                }),
                Rc::new(move || {
                    step_what(cx, store, &c4, d4.clone(), Vec::new(), Some(WhatAct::Task))
                }),
            );
        }
        WhatAct::Next => {
            let mut errors = Vec::new();
            if d.target_and_schema().0.is_none() {
                errors.push("Choose what to run.".to_string());
            }
            if d.form.prompt.trim().is_empty() {
                errors.push("Write the task to run.".into());
            }
            if errors.is_empty() {
                step_when(cx, store, &ctx2, d.clone(), None);
            } else {
                step_what(cx, store, &ctx2, d.clone(), errors, None);
            }
        }
    });
    step_keyed(
        cx,
        ctx,
        step_title(1),
        build,
        act,
        focus,
        back,
        Some(on_key),
    );
}

/// The picker's rows (the kit's workflowPickerRows): "Gateway default —
/// <what it resolves to>", then "<name>  @<version>".
pub fn picker_labels(data: &wp::Executable) -> Vec<String> {
    wp::rows(data)
        .into_iter()
        .map(|r| match (&r.entry, r.detail.is_empty()) {
            (None, _) => format!("{} — {}", r.name, r.detail),
            (Some(_), true) => format!("{} · {}", r.name, group_label(r.group)),
            (Some(_), false) => format!("{}  {} · {}", r.name, r.detail, group_label(r.group)),
        })
        .collect()
}

fn group_label(g: Option<&str>) -> &'static str {
    match g {
        Some("mine") => "Mine",
        _ => "Shared",
    }
}

/// The row the conversation's workflow is (the initial selection).
pub fn conversation_row(data: &wp::Executable, w: &crate::store::Workflow) -> usize {
    if w.gateway_default || w.flow_id.is_empty() {
        return 0;
    }
    wp::rows(data)
        .iter()
        .position(|r| {
            r.entry.as_ref().is_some_and(|e| {
                e.bundle_id == w.bundle_id
                    && e.flow_id == w.flow_id
                    && (w.version.is_empty() || e.bundle_version == w.version)
            })
        })
        .unwrap_or(0)
}

fn pick_workflow(cx: Scope, store: Store, ctx: &UiCtx, d: Draft) {
    let exec = store.automations.with_untracked(|v| v.executable.clone());
    let data = match exec {
        Some(Ok(data)) => data,
        Some(Err(e)) => {
            store.notify(e);
            return;
        }
        None => {
            store.notify("the gateway's workflows are still loading — try again in a moment");
            return;
        }
    };
    let labels = picker_labels(&data);
    if labels.len() <= 1 && matches!(data.gateway_default, wp::GatewayDefault::Unavailable(_)) {
        store.notify(wp::EMPTY);
        return;
    }
    let workflow = store.workflow.get_untracked();
    let conv_row = conversation_row(&data, &workflow);
    let start = match &d.picked {
        None => conv_row,
        Some(p) => wp::rows(&data)
            .iter()
            .position(|r| wp::target_of(r, auto::CODE_AGENT_INTERFACE) == p.target)
            .unwrap_or(conv_row),
    };
    let size = modal_size(110, (labels.len() as i32 + 9).min(32));
    let (ctx2, ctx3, d2, d3) = (ctx.clone(), ctx.clone(), d.clone(), d.clone());
    open_picker(
        cx,
        ctx,
        Picker {
            title: format!("{} · Workflow", step_title(1)),
            labels,
            live: None,
            start,
            size,
            hint: Some("Gateway default: the gateway decides which workflow runs · Enter chooses · Esc goes back".into()),
            live_hint: None,
            keys: Vec::new(),
            on_mount: None,
            on_selection: None,
            on_choose: Box::new(move |ix| {
                let rows = wp::rows(&data);
                let Some(row) = rows.get(ix) else { return };
                let mut d = d2.clone();
                if ix == conv_row && d.conv_target.is_some() {
                    // The conversation's own workflow: its inputs and tools.
                    if d.picked.is_some() {
                        d.tools_set = false;
                    }
                    d.picked = None;
                } else {
                    let schema = wp::schema_key(row, &data);
                    if let Ok((b, v, f)) = &schema {
                        let key = auto::schema_key(b, v, f);
                        if store.automations.with_untracked(|s| s.schema(&key).is_none()) {
                            send(
                                &ctx2,
                                AutoCmd::Schema {
                                    bundle: b.clone(),
                                    version: v.clone(),
                                    flow: f.clone(),
                                },
                            );
                        }
                    }
                    let label = if row.entry.is_none() {
                        format!("Gateway default ({})", row.detail)
                    } else if row.detail.is_empty() {
                        row.name.clone()
                    } else {
                        format!("{} {}", row.name, row.detail)
                    };
                    d.picked = Some(Picked {
                        label,
                        target: wp::target_of(row, auto::CODE_AGENT_INTERFACE),
                        schema,
                    });
                    // Another workflow: the Tools section starts again from
                    // its first value (every tool deselected).
                    d.form.tools = default_tools();
                    d.tools_set = true;
                }
                step_what(cx, store, &ctx2, d, Vec::new(), Some(WhatAct::Workflow));
            }),
            on_cancel: Some(Box::new(move || {
                step_what(
                    cx,
                    store,
                    &ctx3,
                    d3.clone(),
                    Vec::new(),
                    Some(WhatAct::Workflow),
                )
            })),
        },
    );
}

// ---------------------------------------------------------------------------
// 2 · When
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WhenAct {
    Repeat,
    Daily,
    Weekly,
    Monthly,
    Once,
    Email,
    Preset(usize),
    Amount,
    Unit,
    /// A weekly day toggle (`auto::CALENDAR_DAYS` index).
    Day(usize),
    MonthDay,
    At,
    OnceAt,
    EmailEvery,
    EmailBatch,
    EmailFrom,
    EmailDomain,
    EmailTo,
    EmailSubject,
    EmailAttachments,
    Next,
}

fn unit_word(u: char) -> &'static str {
    match u {
        'm' => "minutes",
        'd' => "days",
        _ => "hours",
    }
}

/// The form as it is shown and sent: an email choice falls back to Repeat
/// (24 hours) when the account stops being usable (the kit's `shownKind`).
pub fn shown_form(form: &CreateForm, email_usable: bool) -> CreateForm {
    if matches!(form.when, When::Email) && !email_usable {
        CreateForm {
            when: When::Every {
                amount: "24".into(),
                unit: 'h',
            },
            ..form.clone()
        }
    } else {
        form.clone()
    }
}

/// The trigger the gateway words for this form (every schedule kind, its
/// limits included — the kit's `dialogPreviewTrigger`), or `None` for the
/// email trigger or an incomplete rule.
pub fn preview_trigger(form: &CreateForm, email_usable: bool) -> Option<Value> {
    let shown = shown_form(form, email_usable);
    if matches!(shown.when, When::Email) {
        return None;
    }
    auto::schedule_trigger(&shown).ok()
}

/// Ask the gateway to word `trigger` (once per distinct trigger; nothing
/// stored). The answer lands in `View::preview` (`AutoCmd::Preview`).
pub fn ask_preview(store: Store, ctx: &UiCtx, trigger: &Value) {
    let key = trigger.to_string();
    let asked = store
        .automations
        .with_untracked(|v| v.preview.as_ref().is_some_and(|(k, _)| *k == key));
    if asked {
        return;
    }
    // The gateway has no preview route (probed once this session): the
    // sentence in place, nothing sent again.
    if schedule_api(store) == auto::ScheduleApi::Missing {
        store
            .automations
            .update(|v| v.preview = Some((key, auto::PreviewState::Unavailable)));
        return;
    }
    store
        .automations
        .update(|v| v.preview = Some((key, auto::PreviewState::Loading)));
    send(
        ctx,
        AutoCmd::Preview {
            trigger: trigger.clone(),
        },
    );
}

/// The lines under When / in "Title and limits" for a form: the gateway's
/// own words for every schedule kind (the time-zone line for the
/// wall-clock kinds + `first_run_sentence`, "Checking the schedule…" while
/// it answers, or its refusal); the kit's line for the email trigger;
/// "Incomplete schedule." / "Incomplete email trigger." otherwise.
pub fn when_lines(
    form: &CreateForm,
    email_usable: bool,
    served: Option<&auto::PreviewState>,
) -> Vec<String> {
    let shown = shown_form(form, email_usable);
    if matches!(shown.when, When::Email) {
        let line = auto::schedule_preview(&shown);
        return vec![if line.is_empty() {
            auto::incomplete_line(&shown).to_string()
        } else {
            line
        }];
    }
    if auto::schedule_trigger(&shown).is_err() {
        return vec![auto::incomplete_line(&shown).to_string()];
    }
    auto::preview_lines(
        served.unwrap_or(&auto::PreviewState::Loading),
        shown.when.uses_time_zone(),
    )
}

/// The weekly day toggles: `[x] Mon` rows (state shown by the mark, not by
/// colour), Monday first.
pub fn day_rows(days: &[String]) -> Vec<String> {
    auto::CALENDAR_DAYS
        .iter()
        .map(|d| {
            let on = days.iter().any(|x| x == d);
            format!("{} {}", if on { "[x]" } else { "[ ]" }, auto::day_label(d))
        })
        .collect()
}

/// The monthly day choices: 1–31, then "last" (a day the month lacks runs
/// on its last day).
pub fn month_day_rows() -> Vec<String> {
    let mut rows: Vec<String> = (1..=31).map(|d| d.to_string()).collect();
    rows.push(auto::schedule_text("last_day").to_string());
    rows
}

/// Pure: the When step's cards (`status` = the account's email status;
/// `served` = the gateway's preview of the form's trigger).
pub fn when_cards(
    form: &CreateForm,
    status: Option<&EmailStatus>,
    served: Option<&auto::PreviewState>,
    errors: &[String],
    api: auto::ScheduleApi,
) -> (Vec<Card>, Vec<WhenAct>) {
    let legacy = api == auto::ScheduleApi::Missing;
    let usable = EmailStatus::usable(status);
    let mut cards = vec![Card::heading(auto::schedule_text("legend"))];
    let mut acts = Vec::new();
    // An email choice falls back to Repeat when the account stops being usable.
    let shown = shown_form(form, usable);
    let kind_email = matches!(shown.when, When::Email);
    let kind_once = matches!(shown.when, When::Once { .. });
    let kind_every = matches!(shown.when, When::Every { .. });
    let kinds = [
        (
            kind_every,
            auto::schedule_text("kind_every"),
            WhenAct::Repeat,
        ),
        (
            matches!(shown.when, When::Daily { .. }),
            auto::schedule_text("kind_daily"),
            WhenAct::Daily,
        ),
        (
            matches!(shown.when, When::Weekly { .. }),
            auto::schedule_text("kind_weekly"),
            WhenAct::Weekly,
        ),
        (
            matches!(shown.when, When::Monthly { .. }),
            auto::schedule_text("kind_monthly"),
            WhenAct::Monthly,
        ),
        (kind_once, auto::schedule_text("kind_once"), WhenAct::Once),
    ];
    for (on, label, act) in kinds {
        let calendar = matches!(act, WhenAct::Daily | WhenAct::Weekly | WhenAct::Monthly);
        if legacy && calendar {
            // Shown, marked, not choosable on a gateway without calendar rules.
            cards.push(Card::new(vec![CardLine::new(
                format!("(-) {label} · {}", auto::NEEDS_014_MARK),
                Ink::Faint,
            )]));
        } else {
            cards.push(radio(on, label));
        }
        acts.push(act);
    }
    if usable {
        cards.push(radio(kind_email, email::TRIGGER_LABEL));
    } else {
        // Disabled (the kit's radio with `disabled`): visible, not choosable.
        cards.push(Card::new(vec![CardLine::new(
            format!("(-) {}", email::TRIGGER_LABEL),
            Ink::Faint,
        )]));
    }
    acts.push(WhenAct::Email);
    if !usable {
        cards.push(Card::fixed(vec![CardLine::new(
            email::setup_notice(status),
            Ink::Faint,
        )]));
    }
    match &shown.when {
        When::Email => {
            let f = &form.email;
            let shown_every = if f.every.trim().is_empty() {
                // "60s" is shown as 1 minute (the form offers minutes, hours and days).
                match email::default_every(f.uses_model) {
                    "60s" => "1 minute".to_string(),
                    _ => "1 hour".to_string(),
                }
            } else {
                f.every.trim().to_string()
            };
            cards.push(Card::new(vec![
                CardLine::new(email::EVERY_LABEL, Ink::Text),
                CardLine::new(shown_every, Ink::Text).indent(2),
            ]));
            acts.push(WhenAct::EmailEvery);
            cards.push(Card::note(email::INTERVAL_RULE));
            cards.push(value_card(
                email::MAX_BATCH_LABEL,
                &f.max_batch,
                &email::DEFAULT_MAX_BATCH.to_string(),
            ));
            acts.push(WhenAct::EmailBatch);
            cards.push(Card::note(email::MAX_BATCH_HINT));
            cards.push(Card::heading(email::FILTERS_LEGEND));
            cards.push(value_card(
                email::FROM_IN,
                &f.from_in,
                "alice@example.com, billing@example.org",
            ));
            acts.push(WhenAct::EmailFrom);
            cards.push(value_card(
                email::FROM_DOMAIN_IN,
                &f.from_domain_in,
                "example.com",
            ));
            acts.push(WhenAct::EmailDomain);
            cards.push(value_card(email::TO_IN, &f.to_in, ""));
            acts.push(WhenAct::EmailTo);
            cards.push(value_card(email::SUBJECT_CONTAINS, &f.subject_contains, ""));
            acts.push(WhenAct::EmailSubject);
            cards.push(Card::new(vec![
                CardLine::new(email::HAS_ATTACHMENT, Ink::Text),
                CardLine::new(f.has_attachment.label(), Ink::Text).indent(2),
            ]));
            acts.push(WhenAct::EmailAttachments);
            cards.push(Card::note(email::LIST_HINT));
        }
        When::Once { at } => {
            cards.push(value_card(
                auto::schedule_text("once_label"),
                at,
                "YYYY-MM-DD HH:MM",
            ));
            acts.push(WhenAct::OnceAt);
            cards.push(Card::note(if legacy {
                auto::ONCE_UTC_LEGACY
            } else {
                auto::schedule_text("time_zone_hint")
            }));
        }
        When::Daily { at } | When::Weekly { at, .. } | When::Monthly { at, .. } => {
            if let When::Weekly { days, .. } = &shown.when {
                cards.push(Card::fixed(vec![CardLine::new(
                    auto::schedule_text("days_legend"),
                    Ink::Text,
                )]));
                for (i, row) in day_rows(days).into_iter().enumerate() {
                    let on = row.starts_with("[x]");
                    cards.push(Card::new(vec![CardLine::new(
                        row,
                        if on { Ink::On } else { Ink::Text },
                    )
                    .indent(2)]));
                    acts.push(WhenAct::Day(i));
                }
            }
            if let When::Monthly { day, .. } = &shown.when {
                let shown_day = if day == "last" {
                    auto::schedule_text("last_day").to_string()
                } else {
                    day.clone()
                };
                cards.push(value_card(
                    auto::schedule_text("day_label"),
                    &shown_day,
                    "1–31 or last",
                ));
                acts.push(WhenAct::MonthDay);
            }
            cards.push(value_card(auto::schedule_text("time_label"), at, "HH:MM"));
            acts.push(WhenAct::At);
            cards.push(Card::note(auto::schedule_text("time_zone_hint")));
        }
        When::Every { amount, unit } => {
            let (amount, unit) = (amount.clone(), *unit);
            for (i, (label, n, u)) in WHEN_PRESETS.iter().enumerate() {
                cards.push(radio(amount.trim() == *n && unit == *u, label));
                acts.push(WhenAct::Preset(i));
            }
            cards.push(value_card(
                auto::schedule_text("every_label"),
                &amount,
                "a whole number",
            ));
            acts.push(WhenAct::Amount);
            cards.push(Card::new(vec![CardLine::new(
                ['m', 'h', 'd']
                    .iter()
                    .map(|u| {
                        format!(
                            "{}{}",
                            if *u == unit { "(•) " } else { "( ) " },
                            unit_word(*u)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("  "),
                Ink::Text,
            )]));
            acts.push(WhenAct::Unit);
        }
    }
    // One sentence, once: a refusal that repeats the line in place (the
    // "needs 0.14" sentence) replaces it, in the error ink.
    for line in when_lines(form, usable, served) {
        if !errors.contains(&line) {
            cards.push(Card::fixed(vec![CardLine::new(line, Ink::Faint)]));
        }
    }
    errors_cards(&mut cards, errors);
    cards.push(continue_card(3));
    acts.push(WhenAct::Next);
    (cards, acts)
}

fn step_when(cx: Scope, store: Store, ctx: &UiCtx, d: Draft, focus: Option<WhenAct>) {
    step_when_errors(cx, store, ctx, d, Vec::new(), focus)
}

/// The served preview of `form` from the store (tracked inside a render).
fn served_of(store: Store, trigger: &Option<Value>, tracked: bool) -> Option<auto::PreviewState> {
    let read = |v: &auto::View| trigger.as_ref().and_then(|t| v.preview_for(t).cloned());
    if tracked {
        store.automations.with(read)
    } else {
        store.automations.with_untracked(read)
    }
}

fn step_when_errors(
    cx: Scope,
    store: Store,
    ctx: &UiCtx,
    mut d: Draft,
    errors: Vec<String>,
    focus: Option<WhenAct>,
) {
    // Keep what was picked for the calendar rule across kind switches.
    d.rule.absorb(&d.form.when);
    // Every change reopens the step: ask the gateway to word this form once.
    let trigger = preview_trigger(&d.form, usable(store));
    if let Some(t) = &trigger {
        ask_preview(store, ctx, t);
    }
    let d0 = d.clone();
    let d_back = d.clone();
    let trigger_v = trigger.clone();
    let build: Build<WhenAct> = Rc::new(move || {
        let status = store.automations.with(|v| v.email.clone());
        let served = served_of(store, &trigger_v, true);
        let api = store.automations.with(|v| v.schedule_api);
        when_cards(&d0.form, status.as_ref(), served.as_ref(), &errors, api)
    });
    let ctx2 = ctx.clone();
    let act = Rc::new(move |a: WhenAct| {
        // A change reopens the step with the cursor on the row changed.
        let reopen = {
            let ctx = ctx2.clone();
            let here = a.clone();
            move |d: Draft| step_when(cx, store, &ctx, d, Some(here.clone()))
        };
        let mut nd = d.clone();
        let text_field =
            |title: &str, info: &str, initial: String, apply: fn(&mut CreateForm, String)| {
                let (c3, d3, a3) = (ctx2.clone(), d.clone(), a.clone());
                let (c4, d4, a4) = (ctx2.clone(), d.clone(), a.clone());
                edit_text(
                    cx,
                    &ctx2,
                    format!("{} · {title}", step_title(2)),
                    info,
                    initial,
                    Rc::new(move |v: String| {
                        let mut d = d3.clone();
                        apply(&mut d.form, v.trim().to_string());
                        step_when(cx, store, &c3, d, Some(a3.clone()));
                    }),
                    Rc::new(move || step_when(cx, store, &c4, d4.clone(), Some(a4.clone()))),
                );
            };
        match a {
            WhenAct::Repeat => {
                if !matches!(nd.form.when, When::Every { .. }) {
                    nd.form.when = When::Every {
                        amount: "24".into(),
                        unit: 'h',
                    };
                }
                reopen(nd)
            }
            WhenAct::Daily | WhenAct::Weekly | WhenAct::Monthly
                if schedule_api(store) == auto::ScheduleApi::Missing =>
            {
                // Refused with the one sentence; the picked kind stays.
                step_when_errors(
                    cx,
                    store,
                    &ctx2,
                    d.clone(),
                    vec![auto::NEEDS_NEWER_GATEWAY.to_string()],
                    Some(a.clone()),
                )
            }
            WhenAct::Daily | WhenAct::Weekly | WhenAct::Monthly => {
                let kind = match a {
                    WhenAct::Weekly => "weekly",
                    WhenAct::Monthly => "monthly",
                    _ => "daily",
                };
                nd.form.when = nd.rule.rule(kind);
                reopen(nd)
            }
            WhenAct::Once => {
                if !matches!(nd.form.when, When::Once { .. }) {
                    nd.form.when = When::Once { at: String::new() };
                }
                reopen(nd)
            }
            WhenAct::Email => {
                if !usable(store) {
                    store.notify(email::setup_notice(email_status(store).as_ref()));
                    return;
                }
                nd.form.when = When::Email;
                reopen(nd)
            }
            WhenAct::Preset(i) => {
                let (_, n, u) = WHEN_PRESETS[i];
                nd.form.when = When::Every {
                    amount: n.into(),
                    unit: u,
                };
                reopen(nd)
            }
            WhenAct::Unit => {
                if let When::Every { amount, unit } = &nd.form.when {
                    let next = match unit {
                        'm' => 'h',
                        'h' => 'd',
                        _ => 'm',
                    };
                    nd.form.when = When::Every {
                        amount: amount.clone(),
                        unit: next,
                    };
                }
                reopen(nd)
            }
            WhenAct::Amount => {
                let amount = match &d.form.when {
                    When::Every { amount, .. } => amount.clone(),
                    _ => String::new(),
                };
                text_field(auto::schedule_text("every_label"), "A whole number of at least 1 (the unit is the next row).", amount, |f, v| {
                    let unit = match f.when {
                        When::Every { unit, .. } => unit,
                        _ => 'h',
                    };
                    f.when = When::Every { amount: v, unit };
                })
            }
            WhenAct::Day(i) => {
                if let (When::Weekly { days, at }, Some(day)) =
                    (nd.form.when.clone(), auto::CALENDAR_DAYS.get(i))
                {
                    let mut days = days;
                    if let Some(pos) = days.iter().position(|x| x == day) {
                        days.remove(pos);
                    } else {
                        days.push(day.to_string());
                    }
                    nd.form.when = When::Weekly { days, at };
                    // An emptied day set stays empty (refused on Continue).
                    nd.rule.absorb(&nd.form.when);
                }
                reopen(nd)
            }
            WhenAct::MonthDay => {
                let day = match &d.form.when {
                    When::Monthly { day, .. } => day.clone(),
                    _ => String::new(),
                };
                text_field(
                    auto::schedule_text("day_label"),
                    "A day of the month: 1 to 31, or last (a day the month does not have runs on its last day).",
                    day,
                    |f, v| {
                        if let When::Monthly { at, .. } = f.when.clone() {
                            f.when = When::Monthly { day: v, at };
                        }
                    },
                )
            }
            WhenAct::At => {
                let at = match &d.form.when {
                    When::Daily { at } | When::Weekly { at, .. } | When::Monthly { at, .. } => {
                        at.clone()
                    }
                    _ => String::new(),
                };
                text_field(
                    auto::schedule_text("time_label"),
                    "HH:MM, in your account's time zone.",
                    at,
                    |f, at| {
                        f.when = match f.when.clone() {
                            When::Daily { .. } => When::Daily { at },
                            When::Weekly { days, .. } => When::Weekly { days, at },
                            When::Monthly { day, .. } => When::Monthly { day, at },
                            other => other,
                        };
                    },
                )
            }
            WhenAct::OnceAt => {
                let at = match &d.form.when {
                    When::Once { at } => at.clone(),
                    _ => String::new(),
                };
                let info = if schedule_api(store) == auto::ScheduleApi::Missing {
                    "A date and time read as UTC on this gateway: YYYY-MM-DD HH:MM."
                } else {
                    "A date and time in your account's time zone: YYYY-MM-DD HH:MM."
                };
                text_field(auto::schedule_text("once_label"), info, at, |f, v| {
                    f.when = When::Once { at: v }
                })
            }
            WhenAct::EmailEvery => text_field(
                email::EVERY_LABEL,
                "A whole number and m, h or d (90m, 2h, 1d); empty = the default. The shortest interval is 60 s.",
                d.form.email.every.clone(),
                |f, v| f.email.every = v,
            ),
            WhenAct::EmailBatch => text_field(
                email::MAX_BATCH_LABEL,
                "A whole number from 1 to 1000; empty = 100.",
                d.form.email.max_batch.clone(),
                |f, v| f.email.max_batch = v,
            ),
            WhenAct::EmailFrom => text_field(email::FROM_IN, email::LIST_HINT, d.form.email.from_in.clone(), |f, v| {
                f.email.from_in = v
            }),
            WhenAct::EmailDomain => text_field(
                email::FROM_DOMAIN_IN,
                email::LIST_HINT,
                d.form.email.from_domain_in.clone(),
                |f, v| f.email.from_domain_in = v,
            ),
            WhenAct::EmailTo => text_field(email::TO_IN, email::LIST_HINT, d.form.email.to_in.clone(), |f, v| {
                f.email.to_in = v
            }),
            WhenAct::EmailSubject => text_field(
                email::SUBJECT_CONTAINS,
                "One line of at most 200 characters.",
                d.form.email.subject_contains.clone(),
                |f, v| f.email.subject_contains = v,
            ),
            WhenAct::EmailAttachments => {
                nd.form.email.has_attachment = nd.form.email.has_attachment.next();
                reopen(nd)
            }
            WhenAct::Next => {
                let mut errors = when_errors(&d.form, usable(store));
                // A calendar rule picked before the probe answered.
                if errors.is_empty()
                    && d.form.when.is_calendar()
                    && schedule_api(store) == auto::ScheduleApi::Missing
                {
                    errors.push(auto::NEEDS_NEWER_GATEWAY.to_string());
                }
                // The gateway refused this schedule (e.g. a time already past).
                if errors.is_empty() {
                    if let Some(auto::PreviewState::Failed(e)) = served_of(store, &trigger, false) {
                        errors.push(e);
                    }
                }
                if errors.is_empty() {
                    step_context(cx, store, &ctx2, d.clone(), Vec::new(), None);
                } else {
                    step_when_errors(cx, store, &ctx2, d.clone(), errors, None);
                }
            }
        }
    });
    let back: Rc<dyn Fn()> = {
        let ctx = ctx.clone();
        Rc::new(move || step_what(cx, store, &ctx, d_back.clone(), Vec::new(), None))
    };
    step(cx, ctx, step_title(2), build, act, focus, back);
}

/// The When section's own problems (the kit's sentences), checked before
/// moving on.
pub fn when_errors(form: &CreateForm, email_usable: bool) -> Vec<String> {
    if matches!(form.when, When::Email) && email_usable {
        return email::email_trigger_config_from(&form.email).1;
    }
    let probe = CreateForm {
        start_at: String::new(),
        count: String::new(),
        until: String::new(),
        ..shown_form(form, email_usable)
    };
    auto::schedule_config_form(&probe).err().unwrap_or_default()
}

// ---------------------------------------------------------------------------
// 3 · Context
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContextAct {
    Independent,
    Growing,
    MaxTokens,
    Next,
}

pub fn context_cards(form: &CreateForm, errors: &[String]) -> (Vec<Card>, Vec<ContextAct>) {
    let growing = form.context == "growing";
    let mut cards = vec![Card::heading("Context")];
    let mut acts = Vec::new();
    cards.push(radio(!growing, INDEPENDENT_LABEL));
    acts.push(ContextAct::Independent);
    cards.push(radio(growing, GROWING_LABEL));
    acts.push(ContextAct::Growing);
    if growing {
        cards.push(value_card(
            auto::GROWING_MAX_TOKENS_LABEL,
            &form.growing_max_tokens,
            &auto::DEFAULT_GROWING_MAX_TOKENS.to_string(),
        ));
        acts.push(ContextAct::MaxTokens);
        cards.push(Card::note(auto::GROWING_CONTEXT_HELP));
    }
    errors_cards(&mut cards, errors);
    cards.push(continue_card(4));
    acts.push(ContextAct::Next);
    (cards, acts)
}

fn step_context(
    cx: Scope,
    store: Store,
    ctx: &UiCtx,
    d: Draft,
    errors: Vec<String>,
    focus: Option<ContextAct>,
) {
    let d0 = d.clone();
    let d_back = d.clone();
    let build: Build<ContextAct> = Rc::new(move || context_cards(&d0.form, &errors));
    let ctx2 = ctx.clone();
    let act = Rc::new(move |a: ContextAct| {
        let mut nd = d.clone();
        match a.clone() {
            ContextAct::Independent => {
                nd.form.context = "independent".into();
                step_context(cx, store, &ctx2, nd, Vec::new(), Some(a))
            }
            ContextAct::Growing => {
                nd.form.context = "growing".into();
                step_context(cx, store, &ctx2, nd, Vec::new(), Some(a))
            }
            ContextAct::MaxTokens => {
                let (c3, d3) = (ctx2.clone(), d.clone());
                let (c4, d4) = (ctx2.clone(), d.clone());
                edit_text(
                    cx,
                    &ctx2,
                    format!("{} · {}", step_title(3), auto::GROWING_MAX_TOKENS_LABEL),
                    auto::GROWING_CONTEXT_HELP,
                    d.form.growing_max_tokens.clone(),
                    Rc::new(move |v: String| {
                        let mut d = d3.clone();
                        d.form.growing_max_tokens = v.trim().to_string();
                        step_context(cx, store, &c3, d, Vec::new(), Some(ContextAct::MaxTokens));
                    }),
                    Rc::new(move || {
                        step_context(
                            cx,
                            store,
                            &c4,
                            d4.clone(),
                            Vec::new(),
                            Some(ContextAct::MaxTokens),
                        )
                    }),
                );
            }
            ContextAct::Next => {
                let growing_ok = nd.form.context != "growing"
                    || nd
                        .form
                        .growing_max_tokens
                        .trim()
                        .parse::<u64>()
                        .is_ok_and(|n| n > 0);
                if growing_ok {
                    step_tools(cx, store, &ctx2, nd);
                } else {
                    step_context(
                        cx,
                        store,
                        &ctx2,
                        nd,
                        vec![
                            "Max growing context must be a positive whole number of tokens.".into(),
                        ],
                        None,
                    );
                }
            }
        }
    });
    let back: Rc<dyn Fn()> = {
        let ctx = ctx.clone();
        Rc::new(move || step_when(cx, store, &ctx, d_back.clone(), None))
    };
    step(cx, ctx, step_title(3), build, act, focus, back);
}

// ---------------------------------------------------------------------------
// 4 · Tools
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolsAct {
    DefaultTools,
    SelectAll,
    UnselectAll,
    /// A category header's box: every grantable tool of that toolset on
    /// (none or some were) or off (all were).
    Category(String),
    Tool(String),
    Auto,
    Ask,
    Next,
}

pub const SELECT_ALL: &str = "Select all";
pub const UNSELECT_ALL: &str = "Unselect all";

/// The Tools section's first value in the terminal: every tool
/// deselected and "Use workflow default tools" off (operator ruling
/// 2026-10-09: an automation gets no tool unless you give it one). The
/// web's own first value is [`first_tools`].
pub fn default_tools() -> Option<Vec<String>> {
    Some(Vec::new())
}

/// A category's selection, shown in its header's box: `[x]` all, `[ ]`
/// none, `[~]` some, `[-]` none of its tools can be granted here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tri {
    All,
    None,
    Some,
    Unavailable,
}

impl Tri {
    pub fn marker(self) -> &'static str {
        match self {
            Tri::All => "[x] ",
            Tri::None => "[ ] ",
            Tri::Some => "[~] ",
            Tri::Unavailable => "[-] ",
        }
    }
}

fn grantable<'a>(
    inventory: &'a [crate::store::ToolInfo],
    toolset: Option<&'a str>,
) -> impl Iterator<Item = &'a crate::store::ToolInfo> + 'a {
    inventory
        .iter()
        .filter(move |t| !t.served_disabled && toolset.is_none_or(|ts| t.toolset == ts))
}

/// Pure: a category's state against the selection.
pub fn category_state(
    selected: &[String],
    inventory: &[crate::store::ToolInfo],
    toolset: &str,
) -> Tri {
    let (mut n, mut on) = (0, 0);
    for t in grantable(inventory, Some(toolset)) {
        n += 1;
        if selected.contains(&t.name) {
            on += 1;
        }
    }
    match (n, on) {
        (0, _) => Tri::Unavailable,
        (_, 0) => Tri::None,
        (n, on) if n == on => Tri::All,
        _ => Tri::Some,
    }
}

/// Pure: what a Tools action does to the selection (`None` = "Use
/// workflow default tools"). Served-disabled tools are never added.
pub fn apply_tools(
    tools: Option<Vec<String>>,
    act: &ToolsAct,
    inventory: &[crate::store::ToolInfo],
) -> Option<Vec<String>> {
    let add_all = |list: &mut Vec<String>, toolset: Option<&str>| {
        for t in grantable(inventory, toolset) {
            if !list.contains(&t.name) {
                list.push(t.name.clone());
            }
        }
    };
    match act {
        ToolsAct::DefaultTools => match tools {
            Some(_) => None,
            None => Some(Vec::new()),
        },
        ToolsAct::SelectAll => {
            // The listed tools in the gateway's order, then any selected
            // name this gateway does not list (still visible, removable).
            let mut list = Vec::new();
            add_all(&mut list, None);
            for n in tools.unwrap_or_default() {
                if !list.contains(&n) {
                    list.push(n);
                }
            }
            Some(list)
        }
        ToolsAct::UnselectAll => Some(Vec::new()),
        ToolsAct::Category(ts) => {
            let mut list = tools?;
            if category_state(&list, inventory, ts) == Tri::All {
                list.retain(|n| !grantable(inventory, Some(ts)).any(|t| t.name == *n));
            } else {
                add_all(&mut list, Some(ts));
            }
            Some(list)
        }
        ToolsAct::Tool(name) => {
            let mut list = tools?;
            if let Some(pos) = list.iter().position(|n| n == name) {
                list.remove(pos);
            } else if inventory
                .iter()
                .find(|t| t.name == *name)
                .is_none_or(|t| !t.served_disabled)
            {
                list.push(name.clone());
            }
            Some(list)
        }
        ToolsAct::Auto | ToolsAct::Ask | ToolsAct::Next => tools,
    }
}

/// One run of text on a Tools line; `item` = the action it selects.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolSpan {
    pub text: String,
    pub ink: Ink,
    pub item: Option<usize>,
}

/// One logical line of the Tools step (a one-span line wraps; the button
/// line keeps its spans side by side).
#[derive(Clone, Debug, PartialEq)]
pub struct ToolLine {
    pub spans: Vec<ToolSpan>,
    pub indent: usize,
}

impl ToolLine {
    fn one(text: impl Into<String>, ink: Ink, item: Option<usize>) -> ToolLine {
        ToolLine {
            spans: vec![ToolSpan {
                text: text.into(),
                ink,
                item,
            }],
            indent: 0,
        }
    }

    fn indent(mut self, n: usize) -> ToolLine {
        self.indent = n;
        self
    }
}

/// Pure: the Tools step's lines — "Use workflow default tools", then
/// (with a selection) "Select all · Unselect all", each toolset's header
/// with its tri-state box and its `/tools` rows (same builder), then the
/// approval radios and the kit's hints (the untrusted-content hint only
/// with the email trigger), then Continue. `acts[i]` is what item `i`
/// does.
pub fn tools_lines(
    tools: Option<&[String]>,
    tool_approval: &str,
    inventory: &[crate::store::ToolInfo],
    email_kind: bool,
) -> (Vec<ToolLine>, Vec<ToolsAct>) {
    let mut lines = vec![ToolLine::one("Tools", Ink::Title, None)];
    let mut acts: Vec<ToolsAct> = Vec::new();
    let item = |acts: &mut Vec<ToolsAct>, a: ToolsAct| {
        acts.push(a);
        Some(acts.len() - 1)
    };
    let default_on = tools.is_none();
    let ix = item(&mut acts, ToolsAct::DefaultTools);
    lines.push(ToolLine::one(
        format!(
            "{}{DEFAULT_TOOLS_LABEL}",
            if default_on { "[x] " } else { "[ ] " }
        ),
        if default_on { Ink::On } else { Ink::Text },
        ix,
    ));
    if let Some(selected) = tools {
        let all = item(&mut acts, ToolsAct::SelectAll);
        let none = item(&mut acts, ToolsAct::UnselectAll);
        lines.push(ToolLine {
            spans: vec![
                ToolSpan {
                    text: SELECT_ALL.into(),
                    ink: Ink::Accent,
                    item: all,
                },
                ToolSpan {
                    text: "  ·  ".into(),
                    ink: Ink::Faint,
                    item: None,
                },
                ToolSpan {
                    text: UNSELECT_ALL.into(),
                    ink: Ink::Accent,
                    item: none,
                },
            ],
            indent: 2,
        });
        let on = |n: &str| selected.iter().any(|s| s == n);
        for (row, toolset) in crate::ui::modals::tool_rows(inventory, &on, &[])
            .into_iter()
            .zip(tool_row_toolsets(inventory))
        {
            if row.spec.header {
                let tri = category_state(selected, inventory, &toolset);
                let ix = item(&mut acts, ToolsAct::Category(toolset.clone()));
                lines.push(ToolLine::one(
                    format!("{}{}", tri.marker(), row.spec.text),
                    if tri == Tri::Unavailable {
                        Ink::Faint
                    } else {
                        Ink::Title
                    },
                    ix,
                ));
                continue;
            }
            let marker = row.spec.checked.map(|m| m.marker()).unwrap_or("");
            let ink = if !row.grantable {
                Ink::Faint
            } else if row.name.as_deref().is_some_and(on) {
                Ink::On
            } else {
                Ink::Text
            };
            let ix = item(
                &mut acts,
                ToolsAct::Tool(row.name.clone().unwrap_or_default()),
            );
            lines.push(ToolLine::one(format!("{marker}{}", row.spec.text), ink, ix).indent(2));
        }
        // A selected name this gateway does not list stays in the selection,
        // visible so it can be removed.
        let extra: Vec<&String> = selected
            .iter()
            .filter(|n| !inventory.iter().any(|t| &t.name == *n))
            .collect();
        if !extra.is_empty() {
            lines.push(ToolLine::one(
                "not listed by this gateway",
                Ink::Title,
                None,
            ));
            for n in extra {
                let ix = item(&mut acts, ToolsAct::Tool(n.clone()));
                lines.push(ToolLine::one(format!("[x] {n}"), Ink::On, ix).indent(2));
            }
        }
        if selected.is_empty() {
            lines.push(ToolLine::one("No tools enabled", Ink::Faint, None).indent(2));
        }
    }
    lines.push(ToolLine::one(TOOLS_HINT, Ink::Faint, None));
    let ask = tool_approval == "ask";
    for (on, label, a) in [
        (!ask, AUTO_LABEL, ToolsAct::Auto),
        (ask, ASK_LABEL, ToolsAct::Ask),
    ] {
        let ix = item(&mut acts, a);
        lines.push(ToolLine::one(
            format!("{}{label}", if on { "(•) " } else { "( ) " }),
            if on { Ink::On } else { Ink::Text },
            ix,
        ));
    }
    if email_kind {
        lines.push(ToolLine::one(email::UNTRUSTED_HINT, Ink::Text, None));
    }
    lines.push(ToolLine::one(
        if ask {
            ASK_HINT.to_string()
        } else {
            format!("{}.", auto::TOOL_APPROVAL_CONSENT)
        },
        Ink::Faint,
        None,
    ));
    let ix = item(&mut acts, ToolsAct::Next);
    lines.push(ToolLine::one(
        format!("Continue — {}", STEPS[4]),
        Ink::Accent,
        ix,
    ));
    (lines, acts)
}

/// The toolset of each `tool_rows` row (header rows carry their group's).
fn tool_row_toolsets(inventory: &[crate::store::ToolInfo]) -> Vec<String> {
    let mut out = Vec::new();
    let mut last: Option<&str> = None;
    for t in inventory {
        if last != Some(t.toolset.as_str()) {
            last = Some(t.toolset.as_str());
            out.push(t.toolset.clone());
        }
        out.push(t.toolset.clone());
    }
    out
}

/// One painted row of the Tools list: its cells (column, text, ink,
/// item) and the item the whole row selects (a one-item row).
#[derive(Clone, Debug, PartialEq)]
pub struct ToolRowPaint {
    pub cells: Vec<(usize, String, Ink, Option<usize>)>,
    pub item: Option<usize>,
}

/// Pure: lay the lines out at `width` cells (column 0–1 hold the cursor
/// marker). One-span lines wrap — nothing is cut.
pub fn layout_tool_lines(lines: &[ToolLine], width: usize) -> Vec<ToolRowPaint> {
    let inner = width.saturating_sub(2).max(8);
    let mut rows = Vec::new();
    for line in lines {
        let indent = line.indent.min(inner / 2);
        if let [span] = line.spans.as_slice() {
            for w in crate::ui::cards::wrap(&span.text, inner - indent) {
                rows.push(ToolRowPaint {
                    cells: vec![(2 + indent, w, span.ink, span.item)],
                    item: span.item,
                });
            }
            continue;
        }
        let mut x = 2 + indent;
        let mut cells = Vec::new();
        for s in &line.spans {
            let w = abstracttui::text::width(&s.text).max(0) as usize;
            cells.push((x, s.text.clone(), s.ink, s.item));
            x += w;
        }
        rows.push(ToolRowPaint { cells, item: None });
    }
    rows
}

/// Rows of `item` in a layout (first, last).
fn item_rows(rows: &[ToolRowPaint], item: usize) -> Option<(usize, usize)> {
    let has = |r: &ToolRowPaint| r.cells.iter().any(|c| c.3 == Some(item));
    let first = rows.iter().position(has)?;
    let last = rows.iter().rposition(has).unwrap_or(first);
    Some((first, last))
}

/// Pure: the first visible row — `top` (the last one painted, or the
/// wheel's) when `follow` is off; otherwise the nearest window that shows
/// the cursor's item whole (one row of context where possible).
pub fn tools_window(
    rows: &[ToolRowPaint],
    cursor: usize,
    top: usize,
    height: usize,
    follow: bool,
) -> usize {
    let max_start = rows.len().saturating_sub(height);
    let top = top.min(max_start);
    if !follow {
        return top;
    }
    let Some((first, last)) = item_rows(rows, cursor) else {
        return top;
    };
    if first < top + 1 {
        first.saturating_sub(1).min(max_start)
    } else if last + 2 > top + height {
        (last + 2).saturating_sub(height).min(first).min(max_start)
    } else {
        top
    }
}

/// The Tools step: its own modal (no reopen per change, so the cursor
/// stays on the line you toggled), a scrolling list — ↑↓ / PgUp PgDn /
/// Home End move, the mouse wheel scrolls, a click on a line toggles it
/// (or presses the button / Continue), Space or Enter toggles the focused
/// line.
fn step_tools(cx: Scope, store: Store, ctx: &UiCtx, mut d: Draft) {
    if !d.tools_set {
        d.form.tools = default_tools();
        d.tools_set = true;
    }
    let email_kind = matches!(d.form.when, When::Email) && usable(store);
    let rows_hint = {
        let inventory = store.tools.get_untracked();
        tools_lines(
            d.form.tools.as_deref(),
            &d.form.tool_approval,
            &inventory,
            email_kind,
        )
        .0
        .len() as i32
    };
    let size = modal_size(110, rows_hint + 10);
    let ctx2 = ctx.clone();
    let title = step_title(4);
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let tools = mcx.signal(d.form.tools.clone());
        let approval = mcx.signal(d.form.tool_approval.clone());
        let model = move || {
            let inventory = store.tools.get();
            let list = tools.get();
            let ap = approval.get();
            tools_lines(list.as_deref(), &ap, &inventory, email_kind)
        };
        let start = model().1.len().saturating_sub(1);
        let cursor = mcx.signal(start);
        // The window: `top` = the first painted row (written by the paint,
        // moved by the wheel); `follow` = keep the cursor in view (keys) or
        // not (the wheel scrolls freely). `tick` repaints after a wheel.
        let top = Rc::new(std::cell::Cell::new(0usize));
        let follow = Rc::new(std::cell::Cell::new(true));
        let height = Rc::new(std::cell::Cell::new(10usize));
        let tick = mcx.signal(0u64);
        // What the last paint put where: (y, x0, x1, item).
        let hits: Hits = Rc::default();

        let activate = {
            let ctx = ctx2.clone();
            let d = d.clone();
            Rc::new(move |ix: usize| {
                let (_, acts) = model();
                let Some(a) = acts.get(ix).cloned() else {
                    return;
                };
                let inventory = store.tools.get_untracked();
                match &a {
                    ToolsAct::Next => {
                        let mut nd = d.clone();
                        nd.form.tools = tools.get_untracked();
                        nd.form.tool_approval = approval.get_untracked();
                        step_workspaces(cx, store, &ctx, nd);
                    }
                    ToolsAct::Auto => approval.set("auto".into()),
                    ToolsAct::Ask => approval.set("ask".into()),
                    ToolsAct::Tool(name)
                        if inventory
                            .iter()
                            .any(|t| t.name == *name && t.served_disabled) =>
                    {
                        store.notify(format!("{name} is disabled on this gateway"));
                    }
                    ToolsAct::Category(ts)
                        if category_state(&[], &inventory, ts) == Tri::Unavailable =>
                    {
                        let label = if ts.is_empty() { "other" } else { ts.as_str() };
                        store.notify(format!("{label}: every tool is disabled on this gateway"));
                    }
                    _ => tools.set(apply_tools(tools.get_untracked(), &a, &inventory)),
                }
            })
        };
        let move_cursor = {
            let follow = follow.clone();
            Rc::new(move |delta: i64| {
                let n = model().1.len();
                if n > 0 {
                    follow.set(true);
                    cursor.update(|c| {
                        *c = (*c as i64).saturating_add(delta).clamp(0, n as i64 - 1) as usize
                    });
                }
            })
        };
        let page = {
            let height = height.clone();
            move || (height.get().saturating_sub(2)).max(1) as i64
        };
        let on_mouse = {
            let (top, follow, hits) = (top.clone(), follow.clone(), hits.clone());
            let activate = activate.clone();
            move |ectx: &mut abstracttui::ui::EventCtx, ev: &abstracttui::ui::UiEvent| {
                let abstracttui::ui::UiEvent::Mouse(m) = ev else {
                    return;
                };
                match m.kind {
                    abstracttui::ui::MouseKind::ScrollUp
                    | abstracttui::ui::MouseKind::ScrollDown => {
                        let up = matches!(m.kind, abstracttui::ui::MouseKind::ScrollUp);
                        follow.set(false);
                        top.set(if up {
                            top.get().saturating_sub(3)
                        } else {
                            top.get() + 3
                        });
                        tick.update(|n| *n += 1);
                        ectx.stop_propagation();
                    }
                    abstracttui::ui::MouseKind::Down(abstracttui::ui::MouseButton::Left) => {
                        let hit = hits
                            .borrow()
                            .iter()
                            .find(|(y, x0, x1, _)| *y == m.pos.y && m.pos.x >= *x0 && m.pos.x < *x1)
                            .map(|h| h.3);
                        if let Some(ix) = hit {
                            ectx.stop_propagation();
                            // The row clicked is the row focused (the view
                            // stays where the wheel left it).
                            cursor.set(ix);
                            activate(ix);
                        }
                    }
                    _ => {}
                }
            }
        };
        let list = {
            let (top, follow, height, hits) =
                (top.clone(), follow.clone(), height.clone(), hits.clone());
            dyn_view(
                LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),
                move || {
                    let _ = tick.get();
                    let (lines, acts) = model();
                    let cur = cursor.get().min(acts.len().saturating_sub(1));
                    let (top, follow, height, hits) =
                        (top.clone(), follow.clone(), height.clone(), hits.clone());
                    Element::new()
                        .style(LayoutStyle::column().grow(1.0).basis(Dimension::Cells(0)))
                        .draw(move |canvas, rect| {
                            paint_tools(canvas, rect, &lines, cur, (&top, &follow, &height), &hits)
                        })
                        .build()
                },
            )
        };
        let mv = move_cursor;
        Element::new()
            .style(LayoutStyle::column().padding(Edges::all(1)))
            .focusable()
            .autofocus()
            .on(abstracttui::ui::Phase::Bubble, on_mouse)
            // Esc = back to Context, the tools chosen so far kept.
            .shortcut(KeyChord::plain(Key::Escape), {
                let ctx = ctx2.clone();
                let d = d.clone();
                move |_| {
                    let mut nd = d.clone();
                    nd.form.tools = tools.get_untracked();
                    nd.form.tool_approval = approval.get_untracked();
                    step_context(cx, store, &ctx, nd, Vec::new(), None);
                }
            })
            .shortcut(KeyChord::plain(Key::Up), {
                let mv = mv.clone();
                move |_| mv(-1)
            })
            .shortcut(KeyChord::plain(Key::Down), {
                let mv = mv.clone();
                move |_| mv(1)
            })
            .shortcut(KeyChord::plain(Key::PageUp), {
                let (mv, page) = (mv.clone(), page.clone());
                move |_| mv(-page())
            })
            .shortcut(KeyChord::plain(Key::PageDown), {
                let (mv, page) = (mv.clone(), page.clone());
                move |_| mv(page())
            })
            .shortcut(KeyChord::plain(Key::Home), {
                let mv = mv.clone();
                move |_| mv(i64::MIN / 2)
            })
            .shortcut(KeyChord::plain(Key::End), {
                let mv = mv.clone();
                move |_| mv(i64::MAX / 2)
            })
            .shortcut(KeyChord::plain(Key::Enter), {
                let a = activate.clone();
                move |_| a(cursor.get_untracked())
            })
            .shortcut(KeyChord::plain(Key::Char(' ')), {
                let a = activate.clone();
                move |_| a(cursor.get_untracked())
            })
            .child(title_row(&t, title.clone()))
            .child(list)
            .child(hint_bar(&t, TOOLS_HINTS, 8))
            .build()
    });
}

const TOOLS_HINTS: &[(&str, &str)] = &[
    ("↑↓ PgUp PgDn", ""),
    ("Space", "toggles"),
    ("click", "toggles"),
    ("wheel", "scrolls"),
    ("End", "Continue"),
    ("Esc", "cancels"),
];

/// What the last paint put where: (y, x0, x1, item).
type Hits = Rc<std::cell::RefCell<Vec<(i32, i32, i32, usize)>>>;

/// Paint the Tools list into `rect` and record where each item landed.
fn paint_tools(
    canvas: &mut dyn abstracttui::ui::StyledCanvas,
    rect: Rect,
    lines: &[ToolLine],
    cursor: usize,
    (top, follow, height): (
        &std::cell::Cell<usize>,
        &std::cell::Cell<bool>,
        &std::cell::Cell<usize>,
    ),
    hits: &Hits,
) {
    let t = abstracttui::app::current_theme().tokens;
    let rows = layout_tool_lines(lines, rect.w.max(10) as usize);
    let h = rect.h.max(1) as usize;
    height.set(h);
    let start = tools_window(&rows, cursor, top.get(), h, follow.get());
    top.set(start);
    let mut hit = Vec::new();
    let style_of = |ink: Ink, sel: bool| {
        let fg = if sel {
            t.selection_fg
        } else {
            match ink {
                Ink::Text => t.text,
                Ink::Faint => t.text_faint,
                Ink::Title | Ink::Accent | Ink::On => t.accent,
                Ink::Error => t.error,
            }
        };
        let bg = if sel {
            t.selection_bg
        } else {
            Rgba::TRANSPARENT
        };
        let mut style = abstracttui::render::Style::new().fg(fg).bg(bg);
        if matches!(ink, Ink::Title | Ink::On) {
            style = style.attrs(abstracttui::render::Attrs::BOLD);
        }
        style
    };
    let first_cursor_row = item_rows(&rows, cursor).map(|r| r.0);
    for (line, ri) in (start..rows.len()).take(h).enumerate() {
        let row = &rows[ri];
        let y = rect.y + line as i32;
        let whole = row.item == Some(cursor);
        if whole {
            canvas.fill(
                Rect::new(rect.x, y, rect.w, 1),
                ' ',
                t.selection_fg,
                t.selection_bg,
            );
        }
        if Some(ri) == first_cursor_row {
            canvas.print_styled(Point::new(rect.x, y), "▸ ", &style_of(Ink::Accent, whole));
        }
        if let Some(item) = row.item {
            hit.push((y, rect.x, rect.x + rect.w, item));
        }
        for (x, text, ink, item) in &row.cells {
            let sel = whole || *item == Some(cursor);
            let x = rect.x + *x as i32;
            canvas.print_styled(Point::new(x, y), text, &style_of(*ink, sel));
            if let (Some(item), None) = (item, row.item) {
                let w = abstracttui::text::width(text).max(1);
                hit.push((y, x, x + w, *item));
            }
        }
    }
    // Honest overflow: how many rows sit above / below the window.
    let below = rows.len().saturating_sub(start + h);
    for (n, y, arrow) in [(start, rect.y, "↑"), (below, rect.bottom() - 1, "↓")] {
        if n > 0 {
            let msg = format!(" {arrow} {n} more ");
            let w = abstracttui::text::width(&msg);
            let at = Point::new(rect.x + rect.w - w, y);
            canvas.fill(
                Rect::new(at.x, y, w, 1),
                ' ',
                t.text_faint,
                Rgba::TRANSPARENT,
            );
            canvas.print(at, &msg, t.text_faint, Rgba::TRANSPARENT);
        }
    }
    *hits.borrow_mut() = hit;
}

/// The web's first value of the Tools section (its `initialTools`): from
/// the conversation (its customised list, else the workflow's served
/// default list, `conv_schema` = the conversation workflow's schema) — or
/// "Use workflow default tools" for another workflow. The terminal starts
/// from [`default_tools`] instead (operator ruling 2026-10-09); this stays
/// the parity reference (`tests/schedule_parity.rs`).
pub fn first_tools(d: &Draft, conv_schema: Option<&Value>) -> Option<Vec<String>> {
    if d.picked.is_some() {
        return None;
    }
    let defaults = conv_schema
        .map(|schema| si::schema_defaults(Some(schema)))
        .unwrap_or_default();
    si::initial_tools(&defaults, &d.conv)
}

// ---------------------------------------------------------------------------
// 5 · Workspaces
// ---------------------------------------------------------------------------

/// The dialog's visible "Workspaces" section (R13.2 / R14.4): the kit
/// chooser at the run level, starting from "Use my default"; each change is
/// dry-run by the gateway (a refusal shows its sentence + "Not saved.").
fn step_workspaces(cx: Scope, store: Store, ctx: &UiCtx, d: Draft) {
    store.workspaces.update(|w| {
        w.draft = d.form.workspace.clone();
        if w.status
            .as_ref()
            .is_some_and(|s| s.scope == crate::gateway::workspaces::RUN_SCOPE)
        {
            w.status = None;
        }
    });
    let ctx2 = ctx.clone();
    let cancel_ctx = ctx.clone();
    let d_back = d.clone();
    let next: Rc<dyn Fn()> = Rc::new(move || {
        let mut d = d.clone();
        d.form.workspace = store.workspaces.with_untracked(|w| w.draft.clone());
        step_mailbox(cx, store, &ctx2, d, Vec::new(), None);
    });
    crate::ui::workspace_view::open_screen(
        cx,
        store,
        ctx,
        crate::ui::workspace_view::Host::NewAutomation,
        step_title(5),
        Some((format!("Continue — {}", STEPS[5]), next)),
        // Esc = back to Tools, the workspace chosen so far kept.
        Rc::new(move || {
            let mut d = d_back.clone();
            d.form.workspace = store.workspaces.with_untracked(|w| w.draft.clone());
            step_tools(cx, store, &cancel_ctx, d);
        }),
    );
}

// ---------------------------------------------------------------------------
// 6 · Mailbox
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MailAct {
    Notify,
    OnlyMe,
    List,
    Addresses,
    Next,
}

/// Pure: the Mailbox step's cards (the kit's AfEmailOptionsFields; the
/// switch is unavailable with "Connect a mailbox first." when email is not
/// usable).
pub fn mailbox_cards(
    form: &CreateForm,
    status: Option<&EmailStatus>,
    errors: &[String],
) -> (Vec<Card>, Vec<MailAct>) {
    let usable = EmailStatus::usable(status);
    let mut cards = vec![Card::heading("Mailbox")];
    let mut acts = Vec::new();
    if !usable {
        cards.push(Card::fixed(vec![CardLine::new(
            email::setup_notice(status),
            Ink::Faint,
        )]));
        cards.push(Card::new(vec![CardLine::new(
            format!("[-] {} — {NOT_USABLE_SWITCH}", email::NOTIFY_LABEL),
            Ink::Faint,
        )]));
        acts.push(MailAct::Notify);
    } else {
        cards.push(switch(form.notify_email, email::NOTIFY_LABEL));
        acts.push(MailAct::Notify);
    }
    cards.push(Card::note(email::notify_help()));
    if usable && form.notify_email {
        cards.push(Card::heading(email::RECIPIENTS_LEGEND));
        cards.push(radio(!form.recipients.list, email::RECIPIENTS_SELF));
        acts.push(MailAct::OnlyMe);
        cards.push(radio(form.recipients.list, email::RECIPIENTS_LIST));
        acts.push(MailAct::List);
        if form.recipients.list {
            cards.push(value_card(
                email::RECIPIENTS_LIST,
                &form.recipients.addresses,
                "colleague@example.com",
            ));
            acts.push(MailAct::Addresses);
        }
        cards.push(Card::note(email::RECIPIENTS_HINT));
    }
    errors_cards(&mut cards, errors);
    cards.push(continue_card(7));
    acts.push(MailAct::Next);
    (cards, acts)
}

fn step_mailbox(
    cx: Scope,
    store: Store,
    ctx: &UiCtx,
    d: Draft,
    errors: Vec<String>,
    focus: Option<MailAct>,
) {
    let d0 = d.clone();
    let d_back = d.clone();
    let build: Build<MailAct> = Rc::new(move || {
        let status = store.automations.with(|v| v.email.clone());
        mailbox_cards(&d0.form, status.as_ref(), &errors)
    });
    let ctx2 = ctx.clone();
    let act = Rc::new(move |a: MailAct| {
        let mut nd = d.clone();
        match a.clone() {
            MailAct::Notify => {
                if !usable(store) {
                    store.notify(NOT_USABLE_SWITCH);
                    return;
                }
                nd.form.notify_email = !nd.form.notify_email;
            }
            MailAct::OnlyMe => nd.form.recipients.list = false,
            MailAct::List => nd.form.recipients.list = true,
            MailAct::Addresses => {
                let (c3, d3) = (ctx2.clone(), d.clone());
                let (c4, d4) = (ctx2.clone(), d.clone());
                return edit_text(
                    cx,
                    &ctx2,
                    format!("{} · {}", step_title(6), email::RECIPIENTS_LIST),
                    email::RECIPIENTS_HINT,
                    d.form.recipients.addresses.clone(),
                    Rc::new(move |v: String| {
                        let mut d = d3.clone();
                        d.form.recipients.addresses = v;
                        step_mailbox(cx, store, &c3, d, Vec::new(), Some(MailAct::Addresses));
                    }),
                    Rc::new(move || {
                        step_mailbox(
                            cx,
                            store,
                            &c4,
                            d4.clone(),
                            Vec::new(),
                            Some(MailAct::Addresses),
                        )
                    }),
                );
            }
            MailAct::Next => {
                let errors = if usable(store) && nd.form.notify_email {
                    email::allowed_recipients_from(&nd.form.recipients).1
                } else {
                    Vec::new()
                };
                return if errors.is_empty() {
                    step_limits(cx, store, &ctx2, nd, Vec::new(), None)
                } else {
                    step_mailbox(cx, store, &ctx2, nd, errors, None)
                };
            }
        }
        step_mailbox(cx, store, &ctx2, nd, Vec::new(), Some(a));
    });
    let back: Rc<dyn Fn()> = {
        let ctx = ctx.clone();
        Rc::new(move || step_workspaces(cx, store, &ctx, d_back.clone()))
    };
    step(cx, ctx, step_title(6), build, act, focus, back);
}

// ---------------------------------------------------------------------------
// 7 · Title and limits
// ---------------------------------------------------------------------------

/// One row of the "Title and limits" step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitRow {
    Title,
    Start,
    Count,
    Until,
    Create,
}

/// Pure: the "Title and limits" step's cards (the kit's labels: first run
/// for Repeat; max runs / stop at for Repeat and the calendar rules) and
/// what each selectable card does. `served` = the gateway's preview of the
/// form's trigger (its lines, limits included).
pub fn limits_cards(
    form: &CreateForm,
    email_usable: bool,
    served: Option<&auto::PreviewState>,
    errors: &[String],
) -> (Vec<Card>, Vec<LimitRow>) {
    let mut cards = vec![Card::heading(LIMITS_TITLE)];
    let mut acts = Vec::new();
    cards.push(value_card(
        "Title",
        &form.title,
        "Defaults to the task's first line",
    ));
    acts.push(LimitRow::Title);
    let shown = shown_form(form, email_usable);
    let every = matches!(shown.when, When::Every { .. });
    if every {
        cards.push(value_card(
            "First run at (UTC; empty = now)",
            &form.start_at,
            "now",
        ));
        acts.push(LimitRow::Start);
    }
    if every || shown.when.is_calendar() {
        cards.push(value_card(
            "Stop after this many runs",
            &form.count,
            "no limit",
        ));
        acts.push(LimitRow::Count);
        cards.push(value_card("Stop at (UTC)", &form.until, "no end"));
        acts.push(LimitRow::Until);
    }
    for line in when_lines(form, email_usable, served) {
        cards.push(Card::note(line));
    }
    errors_cards(&mut cards, errors);
    cards.push(Card::new(vec![CardLine::new(
        "Create automation",
        Ink::Accent,
    )
    .right("Enter")]));
    acts.push(LimitRow::Create);
    (cards, acts)
}

fn step_limits(
    cx: Scope,
    store: Store,
    ctx: &UiCtx,
    d: Draft,
    errors: Vec<String>,
    focus: Option<LimitRow>,
) {
    // Max runs / stop at / first run change the trigger: the gateway words the new one.
    let trigger = preview_trigger(&d.form, usable(store));
    if let Some(t) = &trigger {
        ask_preview(store, ctx, t);
    }
    let d0 = d.clone();
    let back: Rc<dyn Fn()> = {
        let ctx = ctx.clone();
        let d = d.clone();
        Rc::new(move || step_mailbox(cx, store, &ctx, d.clone(), Vec::new(), None))
    };
    let build: Build<LimitRow> = Rc::new(move || {
        let ok = store
            .automations
            .with(|v| EmailStatus::usable(v.email.as_ref()));
        let served = served_of(store, &trigger, true);
        limits_cards(&d0.form, ok, served.as_ref(), &errors)
    });
    let ctx2 = ctx.clone();
    let act = Rc::new(move |row: LimitRow| {
        let edit =
            |title: &str, info: &str, initial: String, apply: fn(&mut CreateForm, String)| {
                let (c3, d3) = (ctx2.clone(), d.clone());
                let (c4, d4) = (ctx2.clone(), d.clone());
                edit_text(
                    cx,
                    &ctx2,
                    format!("{} · {title}", step_title(7)),
                    info,
                    initial,
                    Rc::new(move |v: String| {
                        let mut d = d3.clone();
                        apply(&mut d.form, v.trim().to_string());
                        step_limits(cx, store, &c3, d, Vec::new(), Some(row));
                    }),
                    Rc::new(move || step_limits(cx, store, &c4, d4.clone(), Vec::new(), Some(row))),
                );
            };
        match row {
            LimitRow::Title => edit(
                "Title",
                "Defaults to the task's first line (at most 120 characters).",
                d.form.title.clone(),
                |f, v| f.title = v,
            ),
            LimitRow::Start => edit(
                "First run at (UTC; empty = now)",
                "A date and time read as UTC: YYYY-MM-DD HH:MM; empty = now.",
                d.form.start_at.clone(),
                |f, v| f.start_at = v,
            ),
            LimitRow::Count => edit(
                "Stop after this many runs",
                "A whole number of at least 1; empty = no limit.",
                d.form.count.clone(),
                |f, v| f.count = v,
            ),
            LimitRow::Until => edit(
                "Stop at (UTC)",
                "A date and time read as UTC: YYYY-MM-DD HH:MM; empty = no end.",
                d.form.until.clone(),
                |f, v| f.until = v,
            ),
            LimitRow::Create => create_automation(cx, store, &ctx2, &d),
        }
    });
    step(
        cx,
        ctx,
        format!("{} (Enter creates it)", step_title(7)),
        build,
        act,
        focus,
        back,
    );
}

/// The schema answer for `key` as `/automation` needs it: the schema, or the
/// sentence that stops the create.
pub fn schema_for_create(answer: Option<&Result<Value, String>>) -> Result<Value, String> {
    match answer {
        None => Err("Wait for workflow inputs to load.".into()),
        Some(Err(e)) => Err(format!(
            "Workflow inputs could not be checked. Refresh the workflow list and try again. ({e})"
        )),
        Some(Ok(s)) => Ok(s.clone()),
    }
}

/// Pure: the create body for a draft and the schema of its workflow (the
/// web's order: the kit's checks, then the workflow's inputs built and
/// validated), or the sentences that stop it.
pub fn create_body(
    d: &Draft,
    schema: Result<&Value, String>,
    email_usable: bool,
    request_id: &str,
) -> Result<Value, Vec<String>> {
    let (target, _) = d.target_and_schema();
    auto::build_create_request(&d.form, target.clone(), email_usable, request_id)?;
    let schema = schema.map_err(|e| vec![e])?;
    let defaults = si::schema_defaults(Some(schema));
    let built =
        si::automation_input(d.form.prompt.trim(), &defaults, &d.conv).map_err(|e| vec![e])?;
    let supplied: Map<String, Value> = built.as_object().cloned().unwrap_or_default();
    let problems = si::validate_workflow_inputs(Some(schema), &supplied);
    if !problems.is_empty() {
        return Err(vec![problems.join(" ")]);
    }
    auto::schedule_body(&d.form, target, email_usable, &built, request_id)
}

/// "Create automation": the exact create body (one request id per distinct
/// body: a retry of the same body after a transport failure is answered
/// idempotently), then the list opens.
fn create_automation(cx: Scope, store: Store, ctx: &UiCtx, d: &Draft) {
    let email_usable = usable(store);
    let (_, key) = d.target_and_schema();
    let schema = match &key {
        Err(reason) => Err(reason.clone()),
        Ok((b, v, f)) => {
            let answer = store
                .automations
                .with_untracked(|s| s.schema(&auto::schema_key(b, v, f)).cloned());
            if answer.is_none() {
                send(
                    ctx,
                    AutoCmd::Schema {
                        bundle: b.clone(),
                        version: v.clone(),
                        flow: f.clone(),
                    },
                );
            }
            schema_for_create(answer.as_ref())
        }
    };
    match create_body(d, schema.as_ref().map_err(Clone::clone), email_usable, "") {
        Err(errors) => step_limits(cx, store, ctx, d.clone(), errors, None),
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
            let schedule_api = schedule_api(store);
            send(ctx, AutoCmd::Create { body, schedule_api });
            // The list opens now; the new automation opens when the gateway
            // answers (`wire_automations`).
            crate::ui::automations_view::open_automations(cx, store, ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn inv() -> Vec<crate::store::ToolInfo> {
        let t = |n: &str, ts: &str, off: bool| crate::store::ToolInfo {
            name: n.into(),
            toolset: ts.into(),
            served_disabled: off,
            ..Default::default()
        };
        vec![
            t("read_file", "files", false),
            t("write_file", "files", false),
            t("web_search", "web", false),
            t("execute_command", "system", true),
        ]
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_tools_section_starts_with_nothing_selected() {
        assert_eq!(default_tools(), Some(Vec::new()));
        let (lines, acts) = tools_lines(Some(&[]), "auto", &inv(), false);
        assert_eq!(acts[0], ToolsAct::DefaultTools);
        let text: Vec<String> = lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect())
            .collect();
        assert!(
            text.contains(&format!("[ ] {DEFAULT_TOOLS_LABEL}")),
            "{text:?}"
        );
        assert!(text.iter().any(|l| l == "No tools enabled"), "{text:?}");
        assert!(!text.iter().any(|l| l.starts_with("[x]")), "{text:?}");
    }

    #[test]
    fn select_all_and_unselect_all() {
        let all = apply_tools(Some(Vec::new()), &ToolsAct::SelectAll, &inv());
        assert_eq!(all, Some(names(&["read_file", "write_file", "web_search"])));
        // A selected name the gateway does not list is kept (visible).
        let kept = apply_tools(Some(names(&["mcp_x"])), &ToolsAct::SelectAll, &inv());
        assert_eq!(
            kept,
            Some(names(&["read_file", "write_file", "web_search", "mcp_x"]))
        );
        assert_eq!(
            apply_tools(all, &ToolsAct::UnselectAll, &inv()),
            Some(Vec::new())
        );
    }

    #[test]
    fn a_category_box_is_tri_state() {
        let inv = inv();
        assert_eq!(category_state(&[], &inv, "files"), Tri::None);
        assert_eq!(
            category_state(&names(&["read_file"]), &inv, "files"),
            Tri::Some
        );
        let both = names(&["read_file", "write_file"]);
        assert_eq!(category_state(&both, &inv, "files"), Tri::All);
        assert_eq!(category_state(&both, &inv, "system"), Tri::Unavailable);
        let cat = ToolsAct::Category("files".into());
        // some → all, all → none, none → all; other categories untouched.
        let some = Some(names(&["read_file", "web_search"]));
        let all = apply_tools(some, &cat, &inv);
        assert_eq!(all, Some(names(&["read_file", "web_search", "write_file"])));
        let none = apply_tools(all, &cat, &inv);
        assert_eq!(none, Some(names(&["web_search"])));
        assert_eq!(
            apply_tools(none, &cat, &inv),
            Some(names(&["web_search", "read_file", "write_file"]))
        );
        // A gated-only category adds nothing.
        let sys = ToolsAct::Category("system".into());
        assert_eq!(apply_tools(Some(Vec::new()), &sys, &inv), Some(Vec::new()));
        // The header shows the state.
        let (lines, _) = tools_lines(Some(&names(&["read_file"])), "auto", &inv, false);
        let headers: Vec<&str> = lines
            .iter()
            .filter(|l| l.spans.len() == 1 && l.spans[0].ink != Ink::Faint)
            .map(|l| l.spans[0].text.as_str())
            .filter(|t| t.ends_with("files") || t.ends_with("web"))
            .collect();
        assert_eq!(headers, vec!["[~] files", "[ ] web"]);
    }

    #[test]
    fn a_tool_toggle_never_grants_a_disabled_tool() {
        let t = ToolsAct::Tool("execute_command".into());
        assert_eq!(apply_tools(Some(Vec::new()), &t, &inv()), Some(Vec::new()));
        let t = ToolsAct::Tool("read_file".into());
        let on = apply_tools(Some(Vec::new()), &t, &inv());
        assert_eq!(on, Some(names(&["read_file"])));
        assert_eq!(apply_tools(on, &t, &inv()), Some(Vec::new()));
        // "Use workflow default tools": on → null, off → an empty selection.
        let d = ToolsAct::DefaultTools;
        assert_eq!(apply_tools(Some(names(&["read_file"])), &d, &inv()), None);
        assert_eq!(apply_tools(None, &d, &inv()), Some(Vec::new()));
    }

    #[test]
    fn a_toggle_keeps_every_item_where_it_was() {
        // The focus rule rests on this: toggling item N never moves the
        // other items, so the cursor index still names the same line.
        let inv = inv();
        let (_, before) = tools_lines(Some(&[]), "auto", &inv, false);
        for (ix, a) in before.iter().enumerate() {
            if matches!(a, ToolsAct::Next | ToolsAct::DefaultTools) {
                continue;
            }
            let after_sel = apply_tools(Some(Vec::new()), a, &inv);
            let (_, after) = tools_lines(after_sel.as_deref(), "auto", &inv, false);
            assert_eq!(after[ix], before[ix], "item {ix} stays put");
            assert_eq!(after.len(), before.len());
        }
    }

    #[test]
    fn the_window_follows_the_cursor_and_the_wheel_scrolls_freely() {
        let lines: Vec<ToolLine> = (0..40)
            .map(|i| ToolLine::one(format!("tool {i}"), Ink::Text, Some(i)))
            .collect();
        let rows = layout_tool_lines(&lines, 60);
        // Following: the cursor's row is in view.
        let start = tools_window(&rows, 39, 0, 10, true);
        assert!(start + 10 > 39 && start <= 39, "{start}");
        let start = tools_window(&rows, 5, 30, 10, true);
        assert!(start <= 5, "{start}");
        // Free (after a wheel): the top stays, clamped to the end.
        assert_eq!(tools_window(&rows, 39, 3, 10, false), 3);
        assert_eq!(tools_window(&rows, 0, 99, 10, false), 30);
    }

    #[test]
    fn the_button_line_keeps_both_buttons_side_by_side() {
        let (lines, acts) = tools_lines(Some(&[]), "auto", &inv(), false);
        let rows = layout_tool_lines(&lines, 80);
        let buttons = rows
            .iter()
            .find(|r| r.cells.iter().any(|c| c.1 == SELECT_ALL))
            .expect("the button line");
        assert_eq!(buttons.item, None, "two items: no whole-row item");
        let items: Vec<&ToolsAct> = buttons
            .cells
            .iter()
            .filter_map(|c| c.3.map(|i| &acts[i]))
            .collect();
        assert_eq!(items, vec![&ToolsAct::SelectAll, &ToolsAct::UnselectAll]);
    }

    #[test]
    fn a_reopened_step_keeps_the_cursor_on_the_row_changed() {
        let acts = [
            WhenAct::Repeat,
            WhenAct::Weekly,
            WhenAct::Day(1),
            WhenAct::Next,
        ];
        assert_eq!(start_cursor(&acts, None), 3, "first visit: Continue");
        assert_eq!(start_cursor(&acts, Some(&WhenAct::Day(1))), 2);
        // A row that is gone (the kind changed): Continue.
        assert_eq!(start_cursor(&acts, Some(&WhenAct::OnceAt)), 3);
    }

    #[test]
    fn the_dialog_is_named_like_the_webs_new_automation() {
        assert_eq!(step_title(1), "New automation — 1/7 What");
        assert_eq!(step_title(4), "New automation — 4/7 Tools");
    }

    #[test]
    fn a_picked_workflow_starts_from_its_own_tools() {
        let schema = json!({"properties": {"tools": {"default": ["read_file", "web_search"]}}});
        let mut d = Draft {
            opened: Box::default(),
            form: CreateForm::default(),
            conv_target: Some(
                json!({"flow_id": "@default", "interface": auto::CODE_AGENT_INTERFACE}),
            ),
            conv_label: String::new(),
            conv_schema: None,
            picked: None,
            conv: Conversation::default(),
            tools_set: false,
            rule: auto::CalendarRuleState::default(),
        };
        assert_eq!(
            first_tools(&d, Some(&schema)),
            Some(vec!["read_file".to_string(), "web_search".to_string()])
        );
        d.picked = Some(Picked {
            label: "ReAct agent @0.1.0".into(),
            target: json!({"bundle_ref": "react-agent@0.1.0", "flow_id": "react"}),
            schema: Ok(("react-agent".into(), "0.1.0".into(), "react".into())),
        });
        assert_eq!(first_tools(&d, Some(&schema)), None);
    }
}
