//! `/schedule` — the kit's AfScheduleDialog in the terminal (R17.2): seven
//! visible steps, the kit's sections in the kit's order and words (no
//! "Advanced"):
//!
//! 1. What — the workflow ("Gateway default" first; the executable
//!    workflows of `GET /bundles?executable_for=abstractcode.agent.v1`) and
//!    the task;
//! 2. When (UTC) — Repeat (presets, every N minutes/hours/days), Once at…,
//!    or "When an email arrives" (only while `GET /me/email` says the
//!    account can be used; otherwise the kit's "Connect a mailbox first —
//!    open My email");
//! 3. Context — Independent / Growing (+ "Max growing context (tokens)");
//! 4. Tools — the `/tools` rows (initialised from the conversation's tool
//!    choice; "Use workflow default tools") + Run without asking / Ask me;
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
//! Each step is a card list: ↑↓ move, Enter (or Space) changes the selected
//! row or continues; Esc cancels. The cursor starts on "Continue", so
//! Enter-Enter-… creates with the defaults.

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
    "When (UTC)",
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

/// The step title: "New automation — 2/7 When (UTC)".
pub fn step_title(n: usize) -> String {
    format!("New automation — {n}/{} {}", STEPS.len(), STEPS[n - 1])
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

/// `/schedule [task]`.
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
    let draft = Draft {
        form: CreateForm {
            prompt: seed.unwrap_or_else(|| last_prompt(store)),
            ..CreateForm::default()
        },
        conv_target: auto::target_for(&workflow),
        conv_label,
        conv_schema,
        picked: None,
        conv: conversation_of(store),
        tools_set: false,
    };
    step_what(cx, store, ctx, draft, Vec::new());
}

// ---------------------------------------------------------------------------
// The generic step: a card list with a cursor
// ---------------------------------------------------------------------------

const STEP_HINTS: &[(&str, &str)] = &[
    ("↑↓", ""),
    ("Enter", "change / continue"),
    ("Esc", "cancels"),
];

type Build<A> = Rc<dyn Fn() -> (Vec<Card>, Vec<A>)>;

/// Open one step: `build` gives the cards and what each selectable card
/// does (re-read on every frame, so live answers render); `act` runs the
/// selected card's action. The cursor starts on the LAST selectable card
/// (Continue / Create automation).
fn step<A: Clone + 'static>(
    cx: Scope,
    ctx: &UiCtx,
    title: String,
    build: Build<A>,
    act: Rc<dyn Fn(A)>,
) {
    let (cards0, acts0) = build();
    let rows: i32 = cards0.iter().map(|c| c.lines.len() as i32 + 1).sum();
    let size = modal_size(100, rows + 8);
    let ctx2 = ctx.clone();
    let start = acts0.len().saturating_sub(1);
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let cursor = mcx.signal(start);
        let activate = {
            let build = build.clone();
            let act = act.clone();
            Rc::new(move || {
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
        let mv = Rc::new(move_cursor);
        let build_v = build.clone();
        Element::new()
            .style(LayoutStyle::column().padding(Edges::all(1)))
            .focusable()
            .autofocus()
            .shortcut(KeyChord::plain(Key::Escape), {
                let ctx = ctx2.clone();
                move |_| ctx.close_modal()
            })
            .shortcut(KeyChord::plain(Key::Up), {
                let mv = mv.clone();
                move |_| mv(-1)
            })
            .shortcut(KeyChord::plain(Key::Down), {
                let mv = mv.clone();
                move |_| mv(1)
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

fn step_what(cx: Scope, store: Store, ctx: &UiCtx, d: Draft, errors: Vec<String>) {
    let d0 = d.clone();
    let build: Build<WhatAct> = Rc::new(move || {
        let exec = store.automations.with(|v| v.executable.clone());
        what_cards(&d0, exec.as_ref(), &errors)
    });
    let ctx2 = ctx.clone();
    let act = Rc::new(move |a: WhatAct| match a {
        WhatAct::Workflow => pick_workflow(cx, store, &ctx2, d.clone()),
        WhatAct::Task => {
            let (c3, d3) = (ctx2.clone(), d.clone());
            let (c4, d4) = (ctx2.clone(), d.clone());
            edit_text(
                cx,
                &ctx2,
                format!("{} · Task", step_title(1)),
                "What every run is asked to do (sent as the prompt of every run).",
                d.form.prompt.clone(),
                Rc::new(move |v: String| {
                    let mut d = d3.clone();
                    d.form.prompt = v;
                    step_what(cx, store, &c3, d, Vec::new());
                }),
                Rc::new(move || step_what(cx, store, &c4, d4.clone(), Vec::new())),
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
                step_when(cx, store, &ctx2, d.clone());
            } else {
                step_what(cx, store, &ctx2, d.clone(), errors);
            }
        }
    });
    step(cx, ctx, step_title(1), build, act);
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
                    // Another workflow owns its own tools: "Use workflow default tools".
                    d.form.tools = None;
                    d.tools_set = true;
                }
                step_what(cx, store, &ctx2, d, Vec::new());
            }),
            on_cancel: Some(Box::new(move || step_what(cx, store, &ctx3, d3.clone(), Vec::new()))),
        },
    );
}

// ---------------------------------------------------------------------------
// 2 · When (UTC)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WhenAct {
    Repeat,
    Once,
    Email,
    Preset(usize),
    Amount,
    Unit,
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

/// Pure: the When step's cards (`status` = the account's email status).
pub fn when_cards(
    form: &CreateForm,
    status: Option<&EmailStatus>,
    errors: &[String],
) -> (Vec<Card>, Vec<WhenAct>) {
    let usable = EmailStatus::usable(status);
    let mut cards = vec![Card::heading("When (UTC)")];
    let mut acts = Vec::new();
    // An email choice falls back to Repeat when the account stops being usable.
    let kind_email = matches!(form.when, When::Email) && usable;
    let kind_once = matches!(form.when, When::Once { .. });
    let kind_every = !kind_email && !kind_once;
    cards.push(radio(kind_every, "Repeat"));
    acts.push(WhenAct::Repeat);
    cards.push(radio(kind_once, "Once at…"));
    acts.push(WhenAct::Once);
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
    if kind_email {
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
    } else if kind_once {
        let at = match &form.when {
            When::Once { at } => at.clone(),
            _ => String::new(),
        };
        cards.push(value_card("Run once at (UTC)", &at, "YYYY-MM-DD HH:MM"));
        acts.push(WhenAct::OnceAt);
    } else {
        let (amount, unit) = match &form.when {
            When::Every { amount, unit } => (amount.clone(), *unit),
            _ => ("24".to_string(), 'h'),
        };
        for (i, (label, n, u)) in WHEN_PRESETS.iter().enumerate() {
            cards.push(radio(amount.trim() == *n && unit == *u, label));
            acts.push(WhenAct::Preset(i));
        }
        cards.push(value_card("Every", &amount, "a whole number"));
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
    let shown = if matches!(form.when, When::Email) && !usable {
        CreateForm {
            when: When::Every {
                amount: "24".into(),
                unit: 'h',
            },
            ..form.clone()
        }
    } else {
        form.clone()
    };
    let preview = auto::schedule_preview(&shown);
    cards.push(Card::fixed(vec![CardLine::new(
        if preview.is_empty() {
            auto::incomplete_line(&shown).to_string()
        } else {
            preview
        },
        Ink::Faint,
    )]));
    errors_cards(&mut cards, errors);
    cards.push(continue_card(3));
    acts.push(WhenAct::Next);
    (cards, acts)
}

fn step_when(cx: Scope, store: Store, ctx: &UiCtx, d: Draft) {
    step_when_errors(cx, store, ctx, d, Vec::new())
}

fn step_when_errors(cx: Scope, store: Store, ctx: &UiCtx, d: Draft, errors: Vec<String>) {
    let d0 = d.clone();
    let build: Build<WhenAct> = Rc::new(move || {
        let status = store.automations.with(|v| v.email.clone());
        when_cards(&d0.form, status.as_ref(), &errors)
    });
    let ctx2 = ctx.clone();
    let act = Rc::new(move |a: WhenAct| {
        let reopen = {
            let ctx = ctx2.clone();
            move |d: Draft| step_when(cx, store, &ctx, d)
        };
        let mut nd = d.clone();
        let text_field =
            |title: &str, info: &str, initial: String, apply: fn(&mut CreateForm, String)| {
                let (c3, d3) = (ctx2.clone(), d.clone());
                let (c4, d4) = (ctx2.clone(), d.clone());
                edit_text(
                    cx,
                    &ctx2,
                    format!("{} · {title}", step_title(2)),
                    info,
                    initial,
                    Rc::new(move |v: String| {
                        let mut d = d3.clone();
                        apply(&mut d.form, v.trim().to_string());
                        step_when(cx, store, &c3, d);
                    }),
                    Rc::new(move || step_when(cx, store, &c4, d4.clone())),
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
                text_field("Every", "A whole number of at least 1 (the unit is the next row).", amount, |f, v| {
                    let unit = match f.when {
                        When::Every { unit, .. } => unit,
                        _ => 'h',
                    };
                    f.when = When::Every { amount: v, unit };
                })
            }
            WhenAct::OnceAt => {
                let at = match &d.form.when {
                    When::Once { at } => at.clone(),
                    _ => String::new(),
                };
                text_field("Run once at (UTC)", "A date and time read as UTC: YYYY-MM-DD HH:MM.", at, |f, v| {
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
                let errors = when_errors(&d.form, usable(store));
                if errors.is_empty() {
                    step_context(cx, store, &ctx2, d.clone(), Vec::new());
                } else {
                    step_when_errors(cx, store, &ctx2, d.clone(), errors);
                }
            }
        }
    });
    step(cx, ctx, step_title(2), build, act);
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
        when: if matches!(form.when, When::Email) {
            When::Every {
                amount: "24".into(),
                unit: 'h',
            }
        } else {
            form.when.clone()
        },
        ..form.clone()
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

fn step_context(cx: Scope, store: Store, ctx: &UiCtx, d: Draft, errors: Vec<String>) {
    let d0 = d.clone();
    let build: Build<ContextAct> = Rc::new(move || context_cards(&d0.form, &errors));
    let ctx2 = ctx.clone();
    let act = Rc::new(move |a: ContextAct| {
        let mut nd = d.clone();
        match a {
            ContextAct::Independent => {
                nd.form.context = "independent".into();
                step_context(cx, store, &ctx2, nd, Vec::new())
            }
            ContextAct::Growing => {
                nd.form.context = "growing".into();
                step_context(cx, store, &ctx2, nd, Vec::new())
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
                        step_context(cx, store, &c3, d, Vec::new());
                    }),
                    Rc::new(move || step_context(cx, store, &c4, d4.clone(), Vec::new())),
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
                    );
                }
            }
        }
    });
    step(cx, ctx, step_title(3), build, act);
}

// ---------------------------------------------------------------------------
// 4 · Tools
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolsAct {
    DefaultTools,
    Tool(String),
    Auto,
    Ask,
    Next,
}

/// Pure: the Tools step's cards — the `/tools` rows (same builder) under
/// "Use workflow default tools", then the approval radios, the kit's
/// hints (the untrusted-content hint only with the email trigger).
pub fn tools_cards(
    form: &CreateForm,
    inventory: &[crate::store::ToolInfo],
    email_kind: bool,
) -> (Vec<Card>, Vec<ToolsAct>) {
    let mut cards = vec![Card::heading("Tools")];
    let mut acts = Vec::new();
    cards.push(switch(form.tools.is_none(), DEFAULT_TOOLS_LABEL));
    acts.push(ToolsAct::DefaultTools);
    if let Some(selected) = &form.tools {
        let on = |n: &str| selected.iter().any(|s| s == n);
        for row in crate::ui::modals::tool_rows(inventory, &on, &[]) {
            let marker = row.spec.checked.map(|m| m.marker()).unwrap_or("");
            if row.spec.header {
                cards.push(Card::heading(row.spec.text.clone()));
                continue;
            }
            let ink = if !row.grantable {
                Ink::Faint
            } else if row.name.as_deref().is_some_and(on) {
                Ink::On
            } else {
                Ink::Text
            };
            cards.push(Card::new(vec![CardLine::new(
                format!("{marker}{}", row.spec.text),
                ink,
            )
            .indent(2)]));
            acts.push(ToolsAct::Tool(row.name.clone().unwrap_or_default()));
        }
        // A selected name this gateway does not list stays in the selection,
        // visible so it can be removed.
        let extra: Vec<&String> = selected
            .iter()
            .filter(|n| !inventory.iter().any(|t| &t.name == *n))
            .collect();
        if !extra.is_empty() {
            cards.push(Card::heading("not listed by this gateway"));
            for n in extra {
                cards.push(Card::new(vec![
                    CardLine::new(format!("[x] {n}"), Ink::On).indent(2)
                ]));
                acts.push(ToolsAct::Tool(n.clone()));
            }
        }
        if selected.is_empty() {
            cards.push(Card::note("No tools enabled"));
        }
    }
    cards.push(Card::note(TOOLS_HINT));
    let ask = form.tool_approval == "ask";
    cards.push(radio(!ask, AUTO_LABEL));
    acts.push(ToolsAct::Auto);
    cards.push(radio(ask, ASK_LABEL));
    acts.push(ToolsAct::Ask);
    if email_kind {
        cards.push(Card::fixed(vec![CardLine::new(
            email::UNTRUSTED_HINT,
            Ink::Text,
        )]));
    }
    cards.push(Card::note(if ask {
        ASK_HINT.to_string()
    } else {
        format!("{}.", auto::TOOL_APPROVAL_CONSENT)
    }));
    cards.push(continue_card(5));
    acts.push(ToolsAct::Next);
    (cards, acts)
}

/// The Tools section's first value (the web's `initialTools`): from the
/// conversation (its customised list, else the workflow's served default
/// list, `conv_schema` = the conversation workflow's schema) — or "Use
/// workflow default tools" for another workflow.
pub fn first_tools(d: &Draft, conv_schema: Option<&Value>) -> Option<Vec<String>> {
    if d.picked.is_some() {
        return None;
    }
    let defaults = conv_schema
        .map(|schema| si::schema_defaults(Some(schema)))
        .unwrap_or_default();
    si::initial_tools(&defaults, &d.conv)
}

fn initial_tools(store: Store, d: &Draft) -> Option<Vec<String>> {
    let schema = d
        .conv_schema
        .as_ref()
        .and_then(|(b, v, f)| {
            store
                .automations
                .with_untracked(|s| s.schema(&auto::schema_key(b, v, f)).cloned())
        })
        .and_then(Result::ok);
    first_tools(d, schema.as_ref())
}

fn step_tools(cx: Scope, store: Store, ctx: &UiCtx, mut d: Draft) {
    if !d.tools_set {
        d.form.tools = initial_tools(store, &d);
        d.tools_set = true;
    }
    let d0 = d.clone();
    let email_kind = matches!(d.form.when, When::Email) && usable(store);
    let build: Build<ToolsAct> = Rc::new(move || {
        let inventory = store.tools.get();
        tools_cards(&d0.form, &inventory, email_kind)
    });
    let ctx2 = ctx.clone();
    let act = Rc::new(move |a: ToolsAct| {
        let mut nd = d.clone();
        match a {
            ToolsAct::DefaultTools => {
                // The kit's checkbox: on → null; off → an empty selection.
                nd.form.tools = if nd.form.tools.is_some() {
                    None
                } else {
                    Some(Vec::new())
                };
            }
            ToolsAct::Tool(name) => {
                let grantable = store.tools.with_untracked(|tl| {
                    tl.iter()
                        .find(|t| t.name == name)
                        .is_none_or(|t| !t.served_disabled)
                });
                if !grantable {
                    store.notify(format!("{name} is disabled on this gateway"));
                    return;
                }
                if let Some(list) = nd.form.tools.as_mut() {
                    if let Some(pos) = list.iter().position(|n| *n == name) {
                        list.remove(pos);
                    } else {
                        list.push(name);
                    }
                }
            }
            ToolsAct::Auto => nd.form.tool_approval = "auto".into(),
            ToolsAct::Ask => nd.form.tool_approval = "ask".into(),
            ToolsAct::Next => return step_workspaces(cx, store, &ctx2, nd),
        }
        step_tools(cx, store, &ctx2, nd);
    });
    step(cx, ctx, step_title(4), build, act);
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
    let next: Rc<dyn Fn()> = Rc::new(move || {
        let mut d = d.clone();
        d.form.workspace = store.workspaces.with_untracked(|w| w.draft.clone());
        step_mailbox(cx, store, &ctx2, d, Vec::new());
    });
    crate::ui::workspace_view::open_screen(
        cx,
        store,
        ctx,
        crate::ui::workspace_view::Host::NewAutomation,
        step_title(5),
        Some((format!("Continue — {}", STEPS[5]), next)),
        Rc::new(move || cancel_ctx.close_modal()),
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

fn step_mailbox(cx: Scope, store: Store, ctx: &UiCtx, d: Draft, errors: Vec<String>) {
    let d0 = d.clone();
    let build: Build<MailAct> = Rc::new(move || {
        let status = store.automations.with(|v| v.email.clone());
        mailbox_cards(&d0.form, status.as_ref(), &errors)
    });
    let ctx2 = ctx.clone();
    let act = Rc::new(move |a: MailAct| {
        let mut nd = d.clone();
        match a {
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
                        step_mailbox(cx, store, &c3, d, Vec::new());
                    }),
                    Rc::new(move || step_mailbox(cx, store, &c4, d4.clone(), Vec::new())),
                );
            }
            MailAct::Next => {
                let errors = if usable(store) && nd.form.notify_email {
                    email::allowed_recipients_from(&nd.form.recipients).1
                } else {
                    Vec::new()
                };
                return if errors.is_empty() {
                    step_limits(cx, store, &ctx2, nd, Vec::new())
                } else {
                    step_mailbox(cx, store, &ctx2, nd, errors)
                };
            }
        }
        step_mailbox(cx, store, &ctx2, nd, Vec::new());
    });
    step(cx, ctx, step_title(6), build, act);
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

/// Pure: the "Title and limits" step's cards (the kit's labels; the limits
/// only for Repeat) and what each selectable card does.
pub fn limits_cards(
    form: &CreateForm,
    email_usable: bool,
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
    let every = match form.when {
        When::Every { .. } => true,
        When::Email => !email_usable,
        When::Once { .. } => false,
    };
    if every {
        cards.push(value_card(
            "First run at (UTC; empty = now)",
            &form.start_at,
            "now",
        ));
        acts.push(LimitRow::Start);
        cards.push(value_card(
            "Stop after this many runs",
            &form.count,
            "no limit",
        ));
        acts.push(LimitRow::Count);
        cards.push(value_card("Stop at (UTC)", &form.until, "no end"));
        acts.push(LimitRow::Until);
    }
    let preview = if matches!(form.when, When::Email) && !email_usable {
        String::new()
    } else {
        auto::schedule_preview(form)
    };
    if !preview.is_empty() {
        cards.push(Card::note(preview));
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

fn step_limits(cx: Scope, store: Store, ctx: &UiCtx, d: Draft, errors: Vec<String>) {
    let d0 = d.clone();
    let build: Build<LimitRow> = Rc::new(move || {
        let ok = store
            .automations
            .with(|v| EmailStatus::usable(v.email.as_ref()));
        limits_cards(&d0.form, ok, &errors)
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
                        step_limits(cx, store, &c3, d, Vec::new());
                    }),
                    Rc::new(move || step_limits(cx, store, &c4, d4.clone(), Vec::new())),
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
    );
}

/// The schema answer for `key` as `/schedule` needs it: the schema, or the
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
        Err(errors) => step_limits(cx, store, ctx, d.clone(), errors),
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
            send(ctx, AutoCmd::Create { body });
            // The list opens now; the new automation opens when the gateway
            // answers (`wire_automations`).
            crate::ui::automations_view::open_automations(cx, store, ctx);
        }
    }
}
