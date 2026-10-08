//! `/settings` — the Code WUI's right rail, in the terminal (R7.3).
//!
//! A full overlay: the open panel on the left, the rail on the right edge
//! (Activity, Files, Model, Workflow, Workspace, Tools, Skills, Voice —
//! the web's order and names). ←/→ (or Tab, or 1–8) changes panel, ↑/↓
//! moves, Enter changes the selected row, Space flips a `[x]` switch, `d`
//! puts the row back to "Gateway default", Esc closes.
//!
//! The panels follow WHAT IS SELECTED:
//! - the conversation → its run settings (the same knobs `/model`,
//!   `/workflow`, `/workspace`, `/tools`, `/skills` edit; one apply path);
//! - an automation (its `Edit`) → its DEFINITION: every change is saved
//!   through the gateway as a new revision (`PATCH /automations/{id}` with
//!   `expected_revision`), shown as "Revision N" with the web's sentences.
//!
//! Activity groups the work per model step (`Start`, `Step 1`, …; the
//! newest open); for an automation one group per run (`Run #3 · 2 h ago ·
//! completed`), the latest open, its steps read from that run's ledger.

use std::rc::Rc;

use abstracttui::prelude::*;
use serde_json::{json, Value};

use crate::automations::{self as auto, Definition, Occurrence, Summary};
use crate::gateway::rail::RailCmd;
use crate::rail::{self, Binding, Panel, RunSettings, SaveState};
use crate::runner::Cmd;
use crate::store::{Conn, Phase, SkillInfo, Store, ToolInfo, Workflow};
use crate::transcript::Item;
use crate::ui::cards::{draw_cards, hint_bar, note_lines, Card, CardLine, Ink};
use crate::ui::modals::{modal_size, open_picker, title_row, Picker};
use crate::ui::UiCtx;

/// A path as its last segment ("Gateway workspace" when empty).
pub fn short_name(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(if trimmed.is_empty() {
            "Gateway workspace"
        } else {
            trimmed
        })
        .to_string()
}

/// What Enter (or Space / `d`) does on a row.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    None,
    // -- the conversation ------------------------------------------------
    ModelPicker,
    Reasoning,
    Mtp,
    Iterations,
    ContextLimit,
    Stream,
    WorkflowPicker,
    /// "Default for new conversations" — the account's choice kept by the
    /// gateway (R17.1): Enter picks, `d` = the gateway default.
    AccountDefault,
    /// A workspace chooser control (session level for the conversation,
    /// run level for an automation).
    Ws(crate::ui::workspace_view::WsAct),
    Permissions,
    ToggleTool(String),
    ToolsModal,
    ToggleSkill(String),
    Files,
    /// Fold/unfold an Activity group (its key).
    Group(String),
    // -- an automation's definition ---------------------------------------
    AModel,
    AReasoning,
    AMtp,
    AIterations,
    ATokens,
    AInstructions,
    AWorkflow,
    ATitle,
    ATask,
    AEvery,
    AContextMode,
    AApproval,
    /// "Stop after this many runs" / "Stop at (UTC)" (repeating schedules).
    ACount,
    AUntil,
    /// "Email result" (the Mailbox switch) and its recipients.
    ANotify,
    ARecipients,
    AToolsMode,
    AToggleTool(String),
    AToggleSkill(String),
    AFiles,
    /// The voice screen (devices, tests, Read aloud, latency, engines).
    Voice,
    /// Fold/unfold an automation run (its index, run id).
    ARun(u64, String),
}

/// The bound automation as the rail reads it: (summary, definition,
/// occurrences, read error).
pub type BoundAutomation = (Option<Summary>, Option<Definition>, Vec<Occurrence>, String);

/// Everything a panel renders, read from the store in one place.
#[derive(Clone)]
pub struct Snap {
    pub binding: Binding,
    pub connected: bool,
    pub title: String,
    pub default_route: (String, String),
    pub provider: String,
    pub model: String,
    pub reasoning: String,
    pub speculation: Option<Value>,
    pub max_iterations: u32,
    pub context_window: u64,
    pub stream: String,
    pub workflow: Workflow,
    pub workflows: Vec<Workflow>,
    pub workspace_root: Option<String>,
    /// This terminal's session (the conversation's workspaces).
    pub session_id: String,
    /// The workspace choosers' data (`store.workspaces`).
    pub ws: crate::workspaces::WsData,
    pub tools: Vec<ToolInfo>,
    pub tools_error: String,
    pub disabled: Vec<String>,
    pub tier: String,
    pub overrides: Vec<(String, String)>,
    pub skills: Vec<SkillInfo>,
    pub skills_error: String,
    pub skills_loaded: bool,
    pub selected_skills: Vec<String>,
    pub items: Vec<Item>,
    pub live: bool,
    pub waiting: bool,
    pub rail: rail::RailData,
    pub voice_defaults: crate::voice::DefaultsState,
    pub voice_prefs: crate::voice::VoicePrefs,
    /// The bound automation: (summary, definition, occurrences, error).
    pub auto: Option<BoundAutomation>,
    /// The account's default workflow row (R17.1).
    pub account: crate::account_prefs::View,
    /// `GET /me/email` (the Mailbox rows; `None` = unknown, not usable).
    pub email: Option<crate::automation_email::EmailStatus>,
}

fn rd<T: Clone + 'static>(s: Signal<T>, tracked: bool) -> T {
    if tracked {
        s.get()
    } else {
        s.get_untracked()
    }
}

/// Read the store (tracked inside a render, untracked inside a handler).
pub fn snap(store: Store, ctx: &UiCtx, binding: &Binding, tracked: bool) -> Snap {
    let read_fold = |f: &crate::transcript::Fold| (f.items.clone(), f.pending_wait.is_some());
    let (items, waiting) = if tracked {
        store.fold.with(read_fold)
    } else {
        store.fold.with_untracked(read_fold)
    };
    let title = items
        .iter()
        .find_map(|i| match i {
            Item::User { text } => Some(crate::conversations::card_title(text, "")),
            _ => None,
        })
        .unwrap_or_else(|| crate::conversations::card_title("", &store.session_id.get_untracked()));
    let auto = match binding {
        Binding::Conversation => None,
        Binding::Automation(id) => Some(
            rd(store.automations, tracked)
                .detail
                .filter(|d| &d.id == id)
                .map_or((None, None, Vec::new(), String::new()), |d| {
                    (d.summary, d.definition, d.occurrences, d.error)
                }),
        ),
    };
    let phase = rd(store.phase, tracked);
    Snap {
        binding: binding.clone(),
        connected: matches!(rd(store.conn, tracked), Conn::Ok | Conn::Unknown),
        title,
        default_route: rd(store.default_route, tracked),
        provider: rd(store.provider, tracked),
        model: rd(store.model, tracked),
        reasoning: rd(store.reasoning, tracked),
        speculation: rd(store.speculation, tracked),
        max_iterations: rd(store.max_iterations, tracked),
        context_window: rd(store.context_window, tracked),
        stream: {
            let s = rd(store.stream_replies, tracked);
            match s {
                crate::streaming::StreamReplies::GatewayDefault => {
                    crate::streaming::gateway_default_label(
                        rd(store.host_contracts, tracked).and_then(|c| c.streaming_default),
                    )
                }
                other => other.label().to_string(),
            }
        },
        workflow: rd(store.workflow, tracked),
        workflows: rd(store.workflows, tracked),
        workspace_root: ctx.workspace_root.clone(),
        session_id: rd(store.session_id, tracked),
        ws: rd(store.workspaces, tracked),
        tools: rd(store.tools, tracked),
        tools_error: rd(store.tools_error, tracked),
        disabled: rd(store.disabled_tools, tracked),
        tier: rd(store.accepted_tier, tracked),
        overrides: rd(store.tool_overrides, tracked),
        skills: rd(store.skills_catalog, tracked),
        skills_error: rd(store.skills_error, tracked),
        skills_loaded: rd(store.skills_shelf, tracked).is_some(),
        selected_skills: rd(store.selected_skills, tracked),
        waiting,
        items,
        live: !matches!(phase, Phase::Idle),
        rail: rd(store.rail, tracked),
        voice_defaults: rd(store.voice.defaults, tracked),
        voice_prefs: {
            // Re-read when the voice screen saves (its tick).
            let _ = rd(store.voice.tick, tracked);
            crate::ui::voice_view::prefs(ctx)
        },
        auto,
        account: rd(store.account_workflow, tracked),
        email: rd(store.automations, tracked).email,
    }
}

/// "Default for new conversations" (R17.1, the Code web's row): shown when
/// the gateway keeps it (an older gateway: nothing, the choice stays on
/// this computer). Value = the gateway's label verbatim; the help sentence
/// or a broken choice's reason; "Saved." / "Not saved. <sentence>".
fn account_default_card(s: &Snap, cards: &mut Vec<Card>, acts: &mut Vec<Act>) {
    use crate::account_prefs::{self as ap, State};
    match &s.account.state {
        State::Ok(r) => {
            let value = if s.account.busy {
                "Saving…".to_string()
            } else {
                r.current_label()
            };
            let mut lines = vec![row(ap::LABEL, value), faint(r.note())];
            if let Some((error, text)) = &s.account.note {
                lines.push(
                    CardLine::new(text.clone(), if *error { Ink::Error } else { Ink::Faint })
                        .indent(2),
                );
            }
            cards.push(Card::new(lines));
            acts.push(Act::AccountDefault);
        }
        State::Error(e) => cards.push(Card::fixed(vec![
            row(ap::LABEL, ""),
            CardLine::new(e.clone(), Ink::Error).indent(2),
        ])),
        State::Unsupported => cards.push(Card::note(ap::UNSUPPORTED)),
        State::Unknown | State::Loading => {}
    }
}

fn row(text: impl Into<String>, right: impl Into<String>) -> CardLine {
    CardLine::new(text, Ink::Text).right(right)
}

fn faint(text: impl Into<String>) -> CardLine {
    CardLine::new(text, Ink::Faint).indent(2)
}

fn or_default(v: &str, default: &str) -> String {
    if v.trim().is_empty() {
        default.into()
    } else {
        v.into()
    }
}

/// The definition panel's limits ("Stop after this many runs", "Stop at
/// (UTC)": repeating schedules) and Mailbox rows ("Email result", the
/// recipients — offered while `GET /me/email` says email is usable; the
/// kit's notice otherwise, the current state still shown).
fn definition_limits_and_mailbox(
    s: &Snap,
    summary: &Summary,
    def: &Definition,
    cards: &mut Vec<Card>,
    acts: &mut Vec<Act>,
) {
    use crate::automation_email as email;
    let form = auto::revise_form_with(summary, Some(def));
    let mut push = |card: Card, act: Act| {
        if card.selectable {
            acts.push(act);
        }
        cards.push(card);
    };
    if let (Some(count), Some(until)) = (&form.count, &form.until) {
        push(
            Card::new(vec![row(
                "Stop after this many runs",
                if count.is_empty() {
                    "no limit".to_string()
                } else {
                    count.clone()
                },
            )]),
            Act::ACount,
        );
        push(
            Card::new(vec![row(
                "Stop at (UTC)",
                if until.is_empty() {
                    "no end".to_string()
                } else {
                    format!("{until} UTC")
                },
            )]),
            Act::AUntil,
        );
    }
    push(Card::heading("Mailbox"), Act::None);
    let usable = email::EmailStatus::usable(s.email.as_ref());
    let on = form.notify_email.unwrap_or(false);
    let recipients = email::notify_recipients(&def.notify);
    if usable {
        push(
            Card::new(vec![CardLine::new(
                format!(
                    "{}{}",
                    if on { "[x] " } else { "[ ] " },
                    email::NOTIFY_LABEL
                ),
                if on { Ink::On } else { Ink::Text },
            )]),
            Act::ANotify,
        );
        push(Card::note(email::notify_help()), Act::None);
        if on {
            push(
                Card::new(vec![
                    row(
                        email::RECIPIENTS_LEGEND,
                        email::recipients_label(&recipients),
                    ),
                    faint(email::RECIPIENTS_HINT),
                ]),
                Act::ARecipients,
            );
        }
    } else {
        push(
            Card::fixed(vec![CardLine::new(
                email::setup_notice(s.email.as_ref()),
                Ink::Faint,
            )]),
            Act::None,
        );
        push(
            Card::new(vec![CardLine::new(
                format!(
                    "{}{} — Connect a mailbox first.",
                    if on { "[x] " } else { "[-] " },
                    email::NOTIFY_LABEL
                ),
                Ink::Faint,
            )]),
            Act::ANotify,
        );
    }
}

/// The settings of the bound automation, or `None` while it loads.
fn auto_settings(s: &Snap) -> Option<(Summary, Definition, RunSettings)> {
    let (summary, def, _, _) = s.auto.as_ref()?;
    let (summary, def) = (summary.clone()?, def.clone()?);
    let input = def.target.get("input_data").cloned().unwrap_or(json!({}));
    let settings = rail::read_settings(&input);
    Some((summary, def, settings))
}

/// The header lines above a panel: what the panels are bound to.
pub fn binding_lines(s: &Snap) -> Vec<String> {
    match &s.binding {
        Binding::Conversation => vec![format!("Conversation {}", s.title)],
        Binding::Automation(_) => match s.auto.as_ref() {
            Some((Some(summary), Some(def), _, _)) => vec![
                format!("Automation {}", summary.title),
                format!("Revision {}", def.revision),
                rail::save_line(&s.rail.save),
            ],
            Some((_, _, _, err)) if !err.is_empty() => {
                vec![format!("The automation could not be read: {err}")]
            }
            _ => vec!["Loading automation…".into()],
        },
    }
}

/// A panel's cards and what each selectable card does (pure over `Snap`).
/// `open` = Activity groups whose fold state the viewer changed:
/// (key, open).
pub fn panel_cards(
    panel: Panel,
    s: &Snap,
    open: &[(String, bool)],
    now: i64,
) -> (Vec<Card>, Vec<Act>) {
    let mut cards: Vec<Card> = Vec::new();
    let mut acts: Vec<Act> = Vec::new();
    let mut push = |cards: &mut Vec<Card>, card: Card, act: Act| {
        if card.selectable {
            acts.push(act);
        }
        cards.push(card);
    };
    let is_auto = matches!(s.binding, Binding::Automation(_));
    if is_auto
        && auto_settings(s).is_none()
        && !matches!(panel, Panel::Activity | Panel::Files | Panel::Voice)
    {
        push(&mut cards, Card::note("Loading automation…"), Act::None);
        return (cards, acts);
    }
    match panel {
        Panel::Activity => activity_cards(s, open, now, &mut cards, &mut acts),
        Panel::Files => {
            let (title, act) = if is_auto {
                ("Automation folder", Act::AFiles)
            } else {
                ("Files", Act::Files)
            };
            push(
                &mut cards,
                Card::new(vec![
                    row(title, "Enter opens"),
                    faint("Name, size and date of each file; a file opens in the preview."),
                ]),
                act,
            );
        }
        Panel::Model => {
            if let Some((_, _, a)) = auto_settings(s).filter(|_| is_auto) {
                let custom = !a.provider.is_empty();
                push(
                    &mut cards,
                    Card::new(vec![
                        row("Model", if custom { "Custom" } else { "Gateway default" }),
                        faint(if custom {
                            format!("{} · {}", a.provider, a.model)
                        } else {
                            rail::model_default_line(&s.default_route)
                        }),
                    ]),
                    Act::AModel,
                );
                push(
                    &mut cards,
                    Card::new(vec![row(
                        "Reasoning effort",
                        or_default(&a.reasoning, "Gateway default"),
                    )]),
                    Act::AReasoning,
                );
                push(
                    &mut cards,
                    Card::new(vec![row(
                        "MTP depth",
                        rail::speculation_label(a.speculation.as_ref()),
                    )]),
                    Act::AMtp,
                );
                push(&mut cards, Card::heading("Behavior"), Act::None);
                push(
                    &mut cards,
                    Card::new(vec![row(
                        "Iteration limit",
                        or_default(&a.max_iterations, "Workflow default"),
                    )]),
                    Act::AIterations,
                );
                push(
                    &mut cards,
                    Card::new(vec![row(
                        "Context token limit",
                        or_default(&a.max_tokens, "Gateway default"),
                    )]),
                    Act::ATokens,
                );
                push(
                    &mut cards,
                    Card::new(vec![
                        row(
                            "Additional instructions",
                            if a.system.is_empty() {
                                "Gateway default"
                            } else {
                                ""
                            },
                        ),
                        faint(or_default(
                            &a.system,
                            "Project conventions or guidance for this conversation…",
                        )),
                    ]),
                    Act::AInstructions,
                );
            } else {
                let custom = !s.provider.is_empty();
                push(
                    &mut cards,
                    Card::new(vec![
                        row("Model", if custom { "Custom" } else { "Gateway default" }),
                        faint(if custom {
                            if s.model.is_empty() {
                                format!("{} (provider default model)", s.provider)
                            } else {
                                format!("{} · {}", s.provider, s.model)
                            }
                        } else {
                            rail::model_default_line(&s.default_route)
                        }),
                    ]),
                    Act::ModelPicker,
                );
                push(
                    &mut cards,
                    Card::new(vec![row(
                        "Reasoning effort",
                        or_default(&s.reasoning, "Gateway default"),
                    )]),
                    Act::Reasoning,
                );
                push(
                    &mut cards,
                    Card::new(vec![row(
                        "MTP depth",
                        rail::speculation_label(s.speculation.as_ref()),
                    )]),
                    Act::Mtp,
                );
                push(&mut cards, Card::heading("Behavior"), Act::None);
                push(
                    &mut cards,
                    Card::new(vec![row(
                        "Iteration limit",
                        if s.max_iterations == 0 {
                            "Workflow default".to_string()
                        } else {
                            s.max_iterations.to_string()
                        },
                    )]),
                    Act::Iterations,
                );
                push(
                    &mut cards,
                    Card::new(vec![row(
                        "Context token limit",
                        if s.context_window == 0 {
                            "Gateway default".to_string()
                        } else {
                            s.context_window.to_string()
                        },
                    )]),
                    Act::ContextLimit,
                );
                push(
                    &mut cards,
                    Card::new(vec![row("Stream replies", s.stream.clone())]),
                    Act::Stream,
                );
            }
            push(
                &mut cards,
                Card::note("Reasoning and additional instructions depend on the selected workflow and model."),
                Act::None,
            );
        }
        Panel::Workflow => {
            if let Some((summary, def, _)) = auto_settings(s).filter(|_| is_auto) {
                let flow = def
                    .target
                    .get("flow_id")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let bundle = def
                    .target
                    .get("bundle_ref")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let named = s
                    .workflows
                    .iter()
                    .find(|w| w.flow_id == flow && bundle.starts_with(&w.bundle_id))
                    .map(|w| w.label())
                    .unwrap_or_else(|| flow.to_string());
                push(
                    &mut cards,
                    Card::new(vec![row("Workflow", named), faint(bundle.to_string())]),
                    Act::AWorkflow,
                );
                push(
                    &mut cards,
                    Card::new(vec![row("Title", summary.title.clone())]),
                    Act::ATitle,
                );
                if let Some(prompt) = def
                    .target
                    .pointer("/input_data/prompt")
                    .and_then(Value::as_str)
                {
                    push(
                        &mut cards,
                        Card::new(vec![row("Task", ""), faint(prompt.to_string())]),
                        Act::ATask,
                    );
                }
                let email_trigger = crate::automation_email::is_email_trigger(
                    &summary.trigger.source_id,
                    summary.trigger.source_version,
                );
                match summary.trigger.config.get("every").and_then(Value::as_str) {
                    Some(every) if summary.trigger.source_id == "schedule" => push(
                        &mut cards,
                        Card::new(vec![row("Repeat every (UTC)", auto::interval_label(every))]),
                        Act::AEvery,
                    ),
                    Some(every) if email_trigger => push(
                        &mut cards,
                        Card::new(vec![row(
                            crate::automation_email::EVERY_LABEL,
                            auto::interval_label(every),
                        )]),
                        Act::AEvery,
                    ),
                    _ => push(
                        &mut cards,
                        Card::note("This trigger has no interval to change."),
                        Act::None,
                    ),
                }
                if email_trigger {
                    push(
                        &mut cards,
                        Card::note(crate::automation_email::email_trigger_label(
                            &summary.trigger.config,
                        )),
                        Act::None,
                    );
                }
                push(
                    &mut cards,
                    Card::new(vec![
                        row("Context", ""),
                        faint(format!(
                            "{}Independent — each run starts fresh",
                            if summary.context_mode == "growing" {
                                "( ) "
                            } else {
                                "(•) "
                            }
                        )),
                        faint(format!(
                            "{}Growing — each run sees the previous runs",
                            if summary.context_mode == "growing" {
                                "(•) "
                            } else {
                                "( ) "
                            }
                        )),
                    ]),
                    Act::AContextMode,
                );
                let ask = def.tool_approval == "ask";
                push(
                    &mut cards,
                    Card::new(vec![
                        row("Tools", ""),
                        faint(format!(
                            "{}Run without asking",
                            if ask { "( ) " } else { "(•) " }
                        )),
                        faint(format!(
                            "{}Ask before each tool call",
                            if ask { "(•) " } else { "( ) " }
                        )),
                    ]),
                    Act::AApproval,
                );
                definition_limits_and_mailbox(s, &summary, &def, &mut cards, &mut acts);
            } else {
                let w = &s.workflow;
                let value = if w.gateway_default {
                    "Gateway default".to_string()
                } else {
                    w.label()
                };
                let mut lines = vec![row("Workflow", value)];
                if w.gateway_default && !w.bundle_id.is_empty() {
                    lines.push(faint(format!("Gateway default: {}", w.versioned_label())));
                } else if !w.version.is_empty() {
                    lines.push(faint(format!("conversation version {}", w.version)));
                }
                if !w.description.is_empty() {
                    lines.push(faint(w.description.clone()));
                }
                push(&mut cards, Card::new(lines), Act::WorkflowPicker);
                account_default_card(s, &mut cards, &mut acts);
            }
        }
        Panel::Workspace => workspace_cards(s, is_auto, &mut cards, &mut acts),
        Panel::Tools => tools_cards(s, is_auto, &mut cards, &mut acts),
        Panel::Skills => skills_cards(s, is_auto, &mut cards, &mut acts),
        Panel::Voice => {
            if !s.connected {
                push(
                    &mut cards,
                    Card::note("Connect to a gateway to configure voice."),
                    Act::None,
                );
            } else {
                // The voice module's state and wording (R7-W3): the
                // gateway's default routes (`store.voice.defaults`, one
                // reader) and this app's overrides; every change happens on
                // the voice screen (Enter).
                push(&mut cards, Card::heading("Engines"), Act::None);
                push(
                    &mut cards,
                    Card::note("Which engines speak and listen."),
                    Act::None,
                );
                let failed = matches!(s.voice_defaults, crate::voice::DefaultsState::Failed(_));
                let d = s.voice_defaults.value();
                let engine = |kind: crate::voice::VoiceKind, over: String| {
                    let summary = crate::voice::default_summary(d, kind, failed);
                    if !over.is_empty() {
                        format!("{over} (this app)")
                    } else if summary.is_empty() {
                        "Gateway default".to_string()
                    } else {
                        format!("Gateway default · {summary}")
                    }
                };
                push(
                    &mut cards,
                    Card::new(vec![row(
                        "Text → speech",
                        engine(
                            crate::voice::VoiceKind::Tts,
                            s.voice_prefs.tts_override_summary(),
                        ),
                    )]),
                    Act::Voice,
                );
                push(
                    &mut cards,
                    Card::new(vec![row(
                        "Speech → text",
                        engine(
                            crate::voice::VoiceKind::Stt,
                            s.voice_prefs.stt_override_summary(),
                        ),
                    )]),
                    Act::Voice,
                );
                if failed {
                    push(
                        &mut cards,
                        Card::note("The gateway's default voice routes could not be read. Requests still use them."),
                        Act::None,
                    );
                }
                for e in d
                    .into_iter()
                    .flat_map(|d| [d.tts.as_ref(), d.stt.as_ref()])
                    .flatten()
                {
                    if !e.configured && !e.note.is_empty() {
                        push(&mut cards, Card::note(e.note.clone()), Act::None);
                    }
                }
                push(&mut cards, Card::heading("Replies"), Act::None);
                push(
                    &mut cards,
                    Card::new(vec![CardLine::new(
                        format!(
                            "{}Read aloud — Speak each new reply.",
                            if s.voice_prefs.read_aloud {
                                "[x] "
                            } else {
                                "[ ] "
                            }
                        ),
                        if s.voice_prefs.read_aloud {
                            Ink::On
                        } else {
                            Ink::Text
                        },
                    )]),
                    Act::Voice,
                );
                let latency = crate::voice::LATENCY
                    .iter()
                    .find(|(k, _)| *k == s.voice_prefs.quality_preset)
                    .map(|(_, l)| l.to_string())
                    .unwrap_or_else(|| s.voice_prefs.quality_preset.clone());
                push(
                    &mut cards,
                    Card::new(vec![row("Voice latency", latency)]),
                    Act::Voice,
                );
                push(
                    &mut cards,
                    Card::new(vec![row("Devices, tests, volume, language", "Enter opens")]),
                    Act::Voice,
                );
            }
        }
    }
    (cards, acts)
}

fn activity_cards(
    s: &Snap,
    open: &[(String, bool)],
    now: i64,
    cards: &mut Vec<Card>,
    acts: &mut Vec<Act>,
) {
    let is_open = |key: &str, default: bool| {
        open.iter()
            .find(|(k, _)| k == key)
            .map(|(_, o)| *o)
            .unwrap_or(default)
    };
    let group_card = |g: &rail::Group, key: &str, open_now: bool, indent: usize| -> Vec<CardLine> {
        let arrow = if open_now { "▾" } else { "▸" };
        let head = if g.summary.is_empty() {
            format!("{arrow} {}", g.title)
        } else {
            format!("{arrow} {} · {}", g.title, g.summary)
        };
        let ink = match g.status {
            rail::GroupStatus::Failed => Ink::Error,
            rail::GroupStatus::Waiting | rail::GroupStatus::Running => Ink::Accent,
            rail::GroupStatus::Done => Ink::Text,
        };
        let _ = key;
        let mut lines = vec![CardLine::new(head, ink)
            .indent(indent)
            .right(g.status_label())];
        if open_now {
            for l in &g.lines {
                lines.push(CardLine::new(l.clone(), Ink::Faint).indent(indent + 4));
            }
        }
        lines
    };
    match &s.binding {
        Binding::Conversation => {
            let groups = rail::activity_groups(&s.items, s.live, s.waiting);
            if groups.is_empty() {
                cards.push(Card::note("Follow the work as it happens."));
                cards.push(Card::note(
                    "Model steps, tool calls and approvals appear here, one group per step.",
                ));
                return;
            }
            let last = groups.len() - 1;
            for (i, g) in groups.iter().enumerate() {
                let key = format!("c:{}", g.title);
                let o = is_open(&key, i == last);
                cards.push(Card::new(group_card(g, &key, o, 0)));
                acts.push(Act::Group(key));
            }
        }
        Binding::Automation(_) => {
            let Some((_, _, occ, _)) = s.auto.as_ref() else {
                cards.push(Card::note("Loading automation…"));
                return;
            };
            if occ.is_empty() {
                cards.push(Card::note("No runs yet."));
                cards.push(Card::note(
                    "Each run of this automation appears here as one group.",
                ));
                return;
            }
            let mut runs: Vec<&Occurrence> = occ.iter().collect();
            runs.sort_by_key(|o| std::cmp::Reverse(o.index));
            for (i, o) in runs.iter().enumerate() {
                let key = format!("r:{}", o.index);
                let open_now = is_open(&key, i == 0);
                let arrow = if open_now { "▾" } else { "▸" };
                let mut lines = vec![CardLine::new(
                    format!(
                        "{arrow} {}",
                        rail::run_group_title(o.index, &o.fired_at, &o.status, now)
                    ),
                    if o.status == "failed" {
                        Ink::Error
                    } else {
                        Ink::Text
                    },
                )];
                if open_now {
                    if let Some(f) = &o.failure {
                        lines.push(CardLine::new(f.message.clone(), Ink::Error).indent(4));
                    }
                    match s.rail.run_activity.iter().find(|(id, _)| id == &o.run_id) {
                        None | Some((_, None)) => {
                            lines.push(CardLine::new("Loading run activity…", Ink::Faint).indent(4))
                        }
                        Some((_, Some(Err(e)))) => lines.push(
                            CardLine::new(format!("Run activity unavailable: {e}"), Ink::Error)
                                .indent(4),
                        ),
                        Some((_, Some(Ok(items)))) => {
                            let groups = rail::activity_groups(items, false, false);
                            if groups.is_empty() {
                                lines.push(
                                    CardLine::new("This run recorded no steps.", Ink::Faint)
                                        .indent(4),
                                );
                            }
                            for g in &groups {
                                lines.extend(group_card(g, "", true, 4));
                            }
                        }
                    }
                }
                cards.push(Card::new(lines));
                acts.push(Act::ARun(o.index, o.run_id.clone()));
            }
        }
    }
}

/// The chooser this panel shows: the conversation's workspaces (session
/// level) or the bound automation's (run level).
pub fn workspace_host(s: &Snap) -> crate::ui::workspace_view::Host {
    use crate::ui::workspace_view::Host;
    match &s.binding {
        Binding::Conversation => Host::Session(s.session_id.clone()),
        Binding::Automation(id) => Host::Automation(id.clone()),
    }
}

fn workspace_cards(s: &Snap, is_auto: bool, cards: &mut Vec<Card>, acts: &mut Vec<Act>) {
    let host = workspace_host(s);
    let input = auto_settings(s)
        .filter(|_| is_auto)
        .map(|(_, def, _)| def.target.get("input_data").cloned().unwrap_or(json!({})));
    if let Some((summary, _, _)) = auto_settings(s).filter(|_| is_auto) {
        cards.push(Card::fixed(vec![row(
            "Runs work in the automation folder",
            summary
                .workspace_root
                .as_deref()
                .map(short_name)
                .unwrap_or_else(|| "Gateway managed".into()),
        )]));
    } else {
        // The private workspace line stays as it was ("Current workspace session-…").
        cards.push(Card::fixed(vec![row(
            "Current workspace",
            short_name(s.workspace_root.as_deref().unwrap_or("")),
        )]));
    }
    let hv = crate::ui::workspace_view::host_view(&host, &s.ws, s.connected, input.as_ref());
    let (more, more_acts) = crate::ui::workspace_view::chooser_cards(&hv, &s.ws);
    cards.extend(more);
    acts.extend(more_acts.into_iter().map(Act::Ws));
}

/// The command sandbox's sentence (the web's tooltip on the state badge),
/// once under the tools it explains.
fn push_sandbox_sentence(tools: &[&ToolInfo], cards: &mut Vec<Card>) {
    if let Some(sentence) = tools
        .iter()
        .filter_map(|t| t.sandbox.as_ref())
        .map(|sb| sb.sentence.clone())
        .find(|s| !s.is_empty())
    {
        cards.push(Card::note(sentence));
    }
}

/// A process-spawning tool's command-sandbox state on its card: the
/// gateway's words ("Sandboxed to this run's workspaces", "Refused: no
/// command sandbox on this host", "Not sandboxed: …"), nothing for every
/// other tool.
pub fn sandbox_state_line(t: &ToolInfo) -> Option<CardLine> {
    let sb = t.sandbox.as_ref()?;
    let ink = match sb.tone {
        crate::sandbox_line::Tone::Ok => Ink::Accent,
        crate::sandbox_line::Tone::Warn | crate::sandbox_line::Tone::Danger => Ink::Error,
    };
    Some(CardLine::new(sb.label.clone(), ink).indent(4))
}

fn tools_cards(s: &Snap, is_auto: bool, cards: &mut Vec<Card>, acts: &mut Vec<Act>) {
    let available: Vec<&ToolInfo> = s.tools.iter().filter(|t| !t.served_disabled).collect();
    let gated: Vec<&ToolInfo> = s.tools.iter().filter(|t| t.served_disabled).collect();
    if s.tools.is_empty() {
        cards.push(Card::note(if s.tools_error.is_empty() {
            "Loading tools…".to_string()
        } else {
            format!("Tools are unavailable: {}", s.tools_error)
        }));
        return;
    }
    if let Some((_, _, a)) = auto_settings(s).filter(|_| is_auto) {
        let custom = a.tools.is_some();
        let selected = a.tools.clone().unwrap_or_default();
        cards.push(Card::new(vec![
            row("Tools", if custom { "Custom allowlist" } else { "All tools" }),
            faint("Choose available tools and when they should ask you. Gateway restrictions always apply."),
        ]));
        acts.push(Act::AToolsMode);
        let enabled = if custom {
            available
                .iter()
                .filter(|t| selected.contains(&t.name))
                .count()
        } else {
            available.len()
        };
        cards.push(Card::note(format!(
            "{enabled} / {} enabled",
            available.len()
        )));
        if custom && selected.is_empty() {
            cards.push(Card::note("An empty custom selection disables every tool."));
        }
        for t in &available {
            let on = !custom || selected.contains(&t.name);
            let mark = if custom {
                if on {
                    "[x] "
                } else {
                    "[ ] "
                }
            } else {
                "[-] "
            };
            let approval = a
                .approval
                .iter()
                .find(|(n, _)| n == &t.name)
                .map(|(_, v)| if v == "ask" { "Ask" } else { "Approve" })
                .unwrap_or("");
            let mut lines = vec![CardLine::new(
                format!("{mark}{}", t.name),
                if on && custom { Ink::Accent } else { Ink::Text },
            )
            .right(approval)];
            lines.extend(sandbox_state_line(t));
            if !custom {
                lines.push(faint("Choose Custom to pick tools one by one."));
            }
            cards.push(Card::new(lines));
            acts.push(Act::AToggleTool(t.name.clone()));
        }
        push_sandbox_sentence(&available, cards);
    } else {
        let tier = crate::tool_policy::Tier::parse_or_default(&s.tier);
        cards.push(Card::new(vec![
            row("Permissions", tier.label()),
            faint("Saved for this Gateway account and future turns. Permissions never enable an unchecked tool. Explicit Ask overrides still ask, even with permissions: all."),
        ]));
        acts.push(Act::Permissions);
        let enabled = available
            .iter()
            .filter(|t| !s.disabled.contains(&t.name))
            .count();
        cards.push(Card::fixed(vec![
            CardLine::new("Tools", Ink::Title).right(format!("{enabled} / {} enabled", available.len())),
            faint("Choose available tools and when they should ask you. Gateway restrictions always apply."),
        ]));
        for t in &available {
            let on = !s.disabled.contains(&t.name);
            let pin = s
                .overrides
                .iter()
                .find(|(n, _)| n == &t.name)
                .map(|(_, v)| if v == "ask" { "Ask" } else { "Approve" })
                .unwrap_or("");
            let mut lines = vec![CardLine::new(
                format!("{}{}", if on { "[x] " } else { "[ ] " }, t.name),
                if on { Ink::Accent } else { Ink::Text },
            )
            .right(pin)];
            lines.extend(sandbox_state_line(t));
            cards.push(Card::new(lines));
            acts.push(Act::ToggleTool(t.name.clone()));
        }
        push_sandbox_sentence(&available, cards);
        cards.push(Card::new(vec![row("More tool options", "/tools")]));
        acts.push(Act::ToolsModal);
    }
    if !gated.is_empty() {
        cards.push(Card::note(format!(
            "{} tools unavailable under gateway policy",
            gated.len()
        )));
        for t in gated {
            cards.push(Card::fixed(vec![CardLine::new(
                format!("[-] {}", t.name),
                Ink::Faint,
            )
            .indent(2)
            .right("Disabled by the gateway")]));
        }
    }
}

fn skills_cards(s: &Snap, is_auto: bool, cards: &mut Vec<Card>, acts: &mut Vec<Act>) {
    cards.push(Card::heading("Skills"));
    cards.push(Card::note("Choose curated guidance for the next turn. The gateway evaluates trust and requirements when the run starts; this terminal does not load or execute skill scripts."));
    if !s.skills_error.is_empty() {
        cards.push(Card::fixed(vec![CardLine::new(
            format!("Skill selection is unavailable: {}", s.skills_error),
            Ink::Error,
        )]));
        return;
    }
    if !s.skills_loaded {
        cards.push(Card::note("Loading skills from the gateway…"));
        return;
    }
    if s.skills.is_empty() {
        cards.push(Card::note("This gateway offers no skills."));
        return;
    }
    let selected = match auto_settings(s).filter(|_| is_auto) {
        Some((_, _, a)) => a.skills,
        None => s.selected_skills.clone(),
    };
    for k in &s.skills {
        let on = selected.contains(&k.name);
        let mark = if k.blocked {
            "[-] "
        } else if on {
            "[x] "
        } else {
            "[ ] "
        };
        let mut lines = vec![CardLine::new(
            format!("{mark}{}", k.name),
            if on { Ink::Accent } else { Ink::Text },
        )
        .right(if k.trust.is_empty() {
            String::new()
        } else {
            format!("Trust: {}", k.trust)
        })];
        if !k.description.is_empty() {
            lines.push(faint(k.description.clone()));
        }
        if k.blocked {
            lines.push(faint("Blocked by the gateway: Blocked by gateway policy."));
        }
        cards.push(Card::new(lines));
        acts.push(if is_auto {
            Act::AToggleSkill(k.name.clone())
        } else {
            Act::ToggleSkill(k.name.clone())
        });
    }
}

/// The rail's key hints.
pub const RAIL_HINTS: &[(&str, &str)] = &[
    ("←→", "panel"),
    ("↑↓", ""),
    ("Enter", "change"),
    ("space", "switch"),
    ("d", "Gateway default"),
    ("Esc", "closes"),
];

const RAIL_W: i32 = 13;

/// Open the rail on `panel`, bound to `binding`.
pub fn open_rail(cx: Scope, store: Store, ctx: &UiCtx, binding: Binding, panel: Panel) {
    // The revision line is NOT reset here: reopening after a text field or
    // a picker must keep "Not saved: …" / "Saved as revision N" readable.
    let start_cursor = store
        .rail
        .with_untracked(|r| if r.panel == Some(panel) { r.cursor } else { 0 });
    store.rail.update(|r| r.panel = Some(panel));
    // What the panels read: the gateway's policy and voice routes, its
    // catalog (workflows, providers), tools and skills.
    ctx.send(Cmd::Rail(RailCmd::LoadPanels));
    crate::ui::voice_view::load_defaults(store, ctx);
    if store.tools.with_untracked(|t| t.is_empty()) {
        ctx.send(Cmd::LoadTools);
    }
    if store.skills_shelf.with_untracked(|s| s.is_none()) {
        ctx.send(Cmd::LoadSkills);
    }
    let ctx2 = ctx.clone();
    let size = modal_size(160, 48);
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        let current = mcx.signal(panel);
        let cursor = mcx.signal(start_cursor);
        // Remembered so a picker or a text field returns to the same row.
        mcx.effect(move || {
            let c = cursor.get();
            store.rail.update(|r| r.cursor = c);
        });
        let open = mcx.signal(Vec::<(String, bool)>::new());
        let binding2 = binding.clone();
        // Bound to an automation: re-read its definition while the panels
        // are open (owned by the modal scope — nothing ticks once closed).
        if let Binding::Automation(id) = &binding {
            // The Mailbox rows read the account's email status.
            ctx2.send(Cmd::Automations(
                crate::gateway::automations::AutoCmd::EmailStatus,
            ));
            let ctx = ctx2.clone();
            let id = id.clone();
            let _ = abstracttui::reactive::interval(
                mcx,
                std::time::Duration::from_secs(15),
                move || {
                    ctx.send(Cmd::Automations(
                        crate::gateway::automations::AutoCmd::Open { id: id.clone() },
                    ));
                },
            );
        }
        // The Workspace panel's chooser reads the gateway (this
        // conversation's workspaces, or the automation's dry run) — and
        // re-reads after a change elsewhere (the account default).
        {
            let ctx = ctx2.clone();
            let binding = binding.clone();
            mcx.effect(move || {
                if current.get() != Panel::Workspace {
                    return;
                }
                let connected = matches!(store.conn.get(), Conn::Ok | Conn::Unknown);
                let (host, input) = match &binding {
                    Binding::Conversation => (
                        crate::ui::workspace_view::Host::Session(store.session_id.get()),
                        None,
                    ),
                    Binding::Automation(id) => (
                        crate::ui::workspace_view::Host::Automation(id.clone()),
                        crate::ui::workspace_view::automation_input(store, id, true)
                            .map(|(i, _)| i),
                    ),
                };
                if matches!(binding, Binding::Automation(_)) && input.is_none() {
                    return;
                }
                let hv = store.workspaces.with(|w| {
                    crate::ui::workspace_view::host_view(&host, w, connected, input.as_ref())
                });
                crate::ui::workspace_view::ensure_loaded(store, &ctx, &host, &hv);
            });
        }
        // An automation run's activity is read when its group is open.
        {
            let ctx = ctx2.clone();
            let binding = binding.clone();
            mcx.effect(move || {
                if current.get() != Panel::Activity {
                    return;
                }
                let Binding::Automation(id) = &binding else {
                    return;
                };
                let occ = store.automations.with(|v| {
                    v.detail
                        .as_ref()
                        .filter(|d| &d.id == id)
                        .map(|d| d.occurrences.clone())
                        .unwrap_or_default()
                });
                let opened = open.get();
                let newest = occ.iter().map(|o| o.index).max();
                for o in &occ {
                    let key = format!("r:{}", o.index);
                    let is_open = opened
                        .iter()
                        .find(|(k, _)| *k == key)
                        .map(|(_, v)| *v)
                        .unwrap_or(Some(o.index) == newest);
                    let known = store
                        .rail
                        .with_untracked(|r| r.run_activity.iter().any(|(rid, _)| rid == &o.run_id));
                    if is_open && !known {
                        store
                            .rail
                            .update(|r| r.run_activity.push((o.run_id.clone(), None)));
                        ctx.send(Cmd::Rail(RailCmd::LoadRunActivity {
                            run_id: o.run_id.clone(),
                        }));
                    }
                }
            });
        }
        let acts = {
            let ctx = ctx2.clone();
            let binding = binding.clone();
            move || {
                let s = snap(store, &ctx, &binding, false);
                panel_cards(
                    current.get_untracked(),
                    &s,
                    &open.get_untracked(),
                    auto::now_unix(),
                )
                .1
            }
        };
        let selected = {
            let acts = acts.clone();
            move || {
                acts()
                    .get(cursor.get_untracked())
                    .cloned()
                    .unwrap_or(Act::None)
            }
        };
        let go = move |delta: i64| {
            current.update(|p| *p = p.step(delta));
            cursor.set(0);
            store
                .rail
                .update(|r| r.panel = Some(current.get_untracked()));
        };
        let move_cursor = {
            let acts = acts.clone();
            move |delta: i64| {
                let n = acts().len();
                if n > 0 {
                    cursor.update(|c| *c = (*c as i64 + delta).clamp(0, n as i64 - 1) as usize);
                }
            }
        };
        let activate = {
            let ctx = ctx2.clone();
            let binding = binding.clone();
            let selected = selected.clone();
            Rc::new(move |reset: bool| {
                let act = selected();
                run_act(
                    cx,
                    store,
                    &ctx,
                    &binding,
                    current.get_untracked(),
                    act,
                    reset,
                    open,
                );
            })
        };
        let mut el = Element::new()
            .style(LayoutStyle::column().padding(Edges::all(1)))
            .focusable()
            .autofocus()
            .shortcut(KeyChord::plain(Key::Escape), {
                let ctx = ctx2.clone();
                move |_| ctx.close_modal()
            })
            .shortcut(KeyChord::plain(Key::Left), move |_| go(-1))
            .shortcut(KeyChord::plain(Key::Right), move |_| go(1))
            .shortcut(KeyChord::plain(Key::Tab), move |_| go(1))
            .shortcut(KeyChord::plain(Key::Up), {
                let m = move_cursor.clone();
                move |_| m(-1)
            })
            .shortcut(KeyChord::plain(Key::Down), move |_| move_cursor(1))
            .shortcut(KeyChord::plain(Key::Enter), {
                let a = activate.clone();
                move |_| a(false)
            })
            .shortcut(KeyChord::plain(Key::Char(' ')), {
                let a = activate.clone();
                move |_| a(false)
            })
            .shortcut(KeyChord::plain(Key::Char('d')), {
                let a = activate.clone();
                move |_| a(true)
            });
        for (i, p) in Panel::ALL.iter().enumerate() {
            let p = *p;
            el = el.shortcut(
                KeyChord::plain(Key::Char(char::from(b'1' + i as u8))),
                move |_| {
                    current.set(p);
                    cursor.set(0);
                    store.rail.update(|r| r.panel = Some(p));
                },
            );
        }
        el.child(dyn_view(LayoutStyle::line(1).shrink(0.0), move || {
            let t2 = abstracttui::app::current_theme().tokens;
            title_row(&t2, current.get().label().to_string())
        }))
        .child(dyn_view(LayoutStyle::column().shrink(0.0), {
            let ctx = ctx2.clone();
            let binding = binding2.clone();
            move || {
                let t2 = abstracttui::app::current_theme().tokens;
                let s = snap(store, &ctx, &binding, true);
                note_lines(&t2, &binding_lines(&s), 8)
            }
        }))
        .child(
            Element::new()
                .style(
                    LayoutStyle::row()
                        .grow(1.0)
                        .basis(Dimension::Cells(0))
                        .gap(1),
                )
                .child(dyn_view(
                    LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),
                    {
                        let ctx = ctx2.clone();
                        let binding = binding2.clone();
                        move || {
                            let s = snap(store, &ctx, &binding, true);
                            let (cards, acts) =
                                panel_cards(current.get(), &s, &open.get(), auto::now_unix());
                            let cur = cursor.get().min(acts.len().saturating_sub(1));
                            draw_cards(cards, cur)
                        }
                    },
                ))
                .child(dyn_view(
                    LayoutStyle::column()
                        .width(Dimension::Cells(RAIL_W))
                        .shrink(0.0),
                    move || {
                        let t2 = abstracttui::app::current_theme().tokens;
                        let here = current.get();
                        let mut col = Element::new().style(LayoutStyle::column());
                        for (i, p) in Panel::ALL.iter().enumerate() {
                            let on = *p == here;
                            let label =
                                format!("{}{} {}", if on { "▸" } else { " " }, i + 1, p.label());
                            let ink = if on { t2.accent } else { t2.text_faint };
                            col = col.child(
                                Element::new()
                                    .style(LayoutStyle::line(1).shrink(0.0))
                                    .draw(move |canvas, rect| {
                                        let mut style = abstracttui::render::Style::new().fg(ink);
                                        if on {
                                            style = style.attrs(abstracttui::render::Attrs::BOLD);
                                        }
                                        canvas.print_styled(
                                            Point::new(rect.x, rect.y),
                                            &label,
                                            &style,
                                        );
                                    })
                                    .build(),
                            );
                        }
                        col.build()
                    },
                ))
                .build(),
        )
        .child(hint_bar(&t, RAIL_HINTS, 8))
        .build()
    });
}

/// `/settings [panel]` — the rail bound to the conversation.
pub fn open_settings(cx: Scope, store: Store, ctx: &UiCtx, panel: Option<Panel>) {
    let panel = panel
        .or_else(|| store.rail.with_untracked(|r| r.panel))
        .unwrap_or(Panel::Model);
    open_rail(cx, store, ctx, Binding::Conversation, panel);
}

/// An automation's `Edit`: select it and open the rail on its definition
/// (the Workflow panel holds the definition form, like the web's
/// `openAutomationSettings`).
pub fn open_automation_settings(cx: Scope, store: Store, ctx: &UiCtx, id: &str) {
    let open = store
        .automations
        .with_untracked(|v| v.detail.as_ref().map(|d| d.id.clone()));
    if open.as_deref() != Some(id) {
        store.automations.update(|v| v.open(id));
    }
    ctx.send(Cmd::Automations(
        crate::gateway::automations::AutoCmd::Open { id: id.to_string() },
    ));
    store.rail.update(|r| r.save = SaveState::Idle);
    open_rail(
        cx,
        store,
        ctx,
        Binding::Automation(id.to_string()),
        Panel::Workflow,
    );
}

/// Reopen the rail where it was (after a picker or a text field).
fn reopen(cx: Scope, store: Store, ctx: &UiCtx, binding: &Binding, panel: Panel) {
    open_rail(cx, store, ctx, binding.clone(), panel);
}

/// Save one change of the bound automation as a new revision.
fn save_revision(store: Store, ctx: &UiCtx, binding: &Binding, changes: Option<Value>) {
    let Binding::Automation(id) = binding else {
        return;
    };
    let Some(changes) = changes else {
        store
            .rail
            .update(|r| r.save = SaveState::Refused("Nothing changed.".into()));
        return;
    };
    let rev = store.automations.with_untracked(|v| {
        v.detail
            .as_ref()
            .filter(|d| &d.id == id)
            .and_then(|d| d.definition.as_ref().map(|def| def.revision))
    });
    let Some(expected_revision) = rev else { return };
    store.rail.update(|r| r.save = SaveState::Saving);
    ctx.send(Cmd::Rail(RailCmd::SaveRevision {
        id: id.clone(),
        command_id: crate::config::mint_session_id(),
        expected_revision,
        changes,
    }));
}

#[allow(clippy::too_many_arguments)]
fn run_act(
    cx: Scope,
    store: Store,
    ctx: &UiCtx,
    binding: &Binding,
    panel: Panel,
    act: Act,
    reset: bool,
    open: Signal<Vec<(String, bool)>>,
) {
    let s = snap(store, ctx, binding, false);
    let settings = auto_settings(&s);
    // Apply a settings edit to the automation's input and save it.
    let save_settings = |edit: &dyn Fn(&mut RunSettings)| {
        if let Some((_, def, mut a)) = settings.clone() {
            edit(&mut a);
            save_revision(store, ctx, binding, rail::settings_changes(&def.target, &a));
        }
    };
    let text_then = |title: &str, info: &str, initial: String, apply: Rc<dyn Fn(String)>| {
        let back: Rc<dyn Fn()> = {
            let ctx = ctx.clone();
            let binding = binding.clone();
            Rc::new(move || reopen(cx, store, &ctx, &binding, panel))
        };
        let back2 = back.clone();
        crate::ui::automations_view::open_text(
            cx,
            ctx,
            title.to_string(),
            vec![info.to_string()],
            initial,
            Rc::new(move |v: String| {
                apply(v);
                back2();
            }),
            back,
        );
    };
    let pick_then = |title: &str, labels: Vec<String>, start: usize, choose: Rc<dyn Fn(usize)>| {
        let ctx2 = ctx.clone();
        let binding2 = binding.clone();
        let ctx3 = ctx.clone();
        let binding3 = binding.clone();
        let rows = labels.len() as i32;
        open_picker(
            cx,
            ctx,
            Picker {
                title: format!("{title} — ↑↓ · Enter chooses · Esc back"),
                labels,
                live: None,
                start,
                size: modal_size(90, (rows + 6).min(30)),
                hint: None,
                live_hint: None,
                keys: Vec::new(),
                on_mount: None,
                on_selection: None,
                on_choose: Box::new(move |ix| {
                    choose(ix);
                    reopen(cx, store, &ctx2, &binding2, panel);
                }),
                on_cancel: Some(Box::new(move || reopen(cx, store, &ctx3, &binding3, panel))),
            },
        );
    };
    // The pickers of the conversation's own commands come back here.
    let return_here = || {
        store
            .rail
            .update(|r| r.return_to = Some((binding.clone(), panel)));
    };
    match act {
        Act::None => {}
        Act::Group(key) => toggle_group(open, &key, &s, panel),
        Act::ARun(index, _) => toggle_group(open, &format!("r:{index}"), &s, panel),
        // -- the conversation --------------------------------------------
        Act::ModelPicker if reset => crate::ui::modals::apply_route(store, ctx, "", ""),
        Act::ModelPicker => {
            return_here();
            crate::ui::modals::open_model_picker(cx, store, ctx);
        }
        Act::Reasoning if reset => crate::ui::modals::apply_reasoning_public(store, ctx, ""),
        Act::Reasoning => {
            return_here();
            crate::ui::modals::open_reasoning_stage(cx, store, ctx);
        }
        Act::Mtp if reset => crate::ui::modals::apply_speculation(store, ctx, None),
        Act::Mtp => {
            return_here();
            crate::ui::modals::open_mtp_stage(cx, store, ctx);
        }
        Act::Iterations if reset => crate::ui::set_max_iterations(store, ctx, Some("off".into())),
        Act::Iterations => {
            let ctx2 = ctx.clone();
            text_then(
                "Iteration limit",
                "A whole number of iterations for new runs; empty or 0 = the workflow default.",
                if s.max_iterations == 0 {
                    String::new()
                } else {
                    s.max_iterations.to_string()
                },
                Rc::new(move |v: String| {
                    let v = v.trim().to_string();
                    crate::ui::set_max_iterations(
                        store,
                        &ctx2,
                        Some(if v.is_empty() { "off".into() } else { v }),
                    );
                }),
            )
        }
        Act::ContextLimit if reset => crate::ui::set_context_window(store, ctx, Some("off".into())),
        Act::ContextLimit => {
            let ctx2 = ctx.clone();
            text_then(
                "Context token limit",
                "The context window in tokens for new runs; empty or 0 = the gateway default.",
                if s.context_window == 0 {
                    String::new()
                } else {
                    s.context_window.to_string()
                },
                Rc::new(move |v: String| {
                    let v = v.trim().to_string();
                    crate::ui::set_context_window(
                        store,
                        &ctx2,
                        Some(if v.is_empty() { "off".into() } else { v }),
                    );
                }),
            )
        }
        Act::Stream if reset => crate::ui::modals::apply_stream_replies(
            store,
            ctx,
            crate::streaming::StreamReplies::GatewayDefault,
        ),
        Act::Stream => {
            return_here();
            crate::ui::modals::open_stream_stage(cx, store, ctx);
        }
        Act::WorkflowPicker => {
            return_here();
            ctx.send(Cmd::LoadCatalog {
                preferred_bundle: None,
                preferred_flow: None,
            });
            crate::ui::modals::open_workflow_picker(cx, store, ctx);
        }
        // R17.1: one PUT per change ("Saved." / "Not saved. <sentence>").
        Act::AccountDefault if reset => crate::ui::account_workflow::save(store, ctx, None),
        Act::AccountDefault => {
            let Some(r) = s.account.row().cloned() else {
                return;
            };
            if s.account.busy {
                return;
            }
            let options = r.options();
            let start = options.iter().position(|(v, _)| *v == r.value).unwrap_or(0);
            let labels = options.iter().map(|(_, l)| l.clone()).collect();
            let ctx2 = ctx.clone();
            pick_then(
                crate::account_prefs::LABEL,
                labels,
                start,
                Rc::new(move |ix| {
                    if let Some((v, _)) = options.get(ix) {
                        if *v != r.value {
                            crate::ui::account_workflow::save(store, &ctx2, v.clone());
                        }
                    }
                }),
            );
        }
        Act::Ws(a) => {
            let host = workspace_host(&s);
            let back: Rc<dyn Fn()> = {
                let ctx = ctx.clone();
                let binding = binding.clone();
                Rc::new(move || reopen(cx, store, &ctx, &binding, panel))
            };
            // `d` on the switch = back to the default (the kit's follow ON).
            let a = if reset {
                crate::ui::workspace_view::WsAct::Follow
            } else {
                a
            };
            if reset {
                let hv = crate::ui::workspace_view::current(store, &host);
                if hv.state.as_ref().is_some_and(|st| !st.policy.configured) {
                    return;
                }
            }
            crate::ui::workspace_view::run_ws_act(cx, store, ctx, &host, a, back);
        }
        Act::Permissions => crate::ui::cycle_permissions(store, ctx),
        Act::ToolsModal => {
            return_here();
            crate::ui::modals::open_tools(cx, store, ctx);
        }
        Act::ToggleTool(name) => {
            let mut disabled = s.disabled.clone();
            if let Some(p) = disabled.iter().position(|d| *d == name) {
                disabled.remove(p);
            } else {
                disabled.push(name);
            }
            store.disabled_tools.set(disabled.clone());
            crate::ui::persist_tool_prefs(store, ctx, |p| p.disabled_tools = disabled.clone());
        }
        Act::ToggleSkill(name) => {
            if s.skills.iter().any(|k| k.name == name && k.blocked) {
                return;
            }
            let mut selected = s.selected_skills.clone();
            if let Some(p) = selected.iter().position(|x| *x == name) {
                selected.remove(p);
            } else {
                selected.push(name);
            }
            store.selected_skills.set(selected.clone());
            crate::ui::persist_prefs(ctx, |p| p.skills = selected.clone());
        }
        Act::Voice => {
            return_here();
            crate::ui::voice_view::open_voice_settings(cx, store, ctx);
        }
        Act::Files => {
            return_here();
            crate::ui::modals::open_files(cx, store, ctx);
        }
        Act::AFiles => {
            if let Binding::Automation(id) = binding {
                let back: Rc<dyn Fn()> = {
                    let ctx = ctx.clone();
                    let binding = binding.clone();
                    Rc::new(move || reopen(cx, store, &ctx, &binding, panel))
                };
                crate::ui::modals::open_run_files(
                    cx,
                    store,
                    ctx,
                    id.clone(),
                    "Automation folder".into(),
                    Some(back),
                );
            }
        }
        // -- the automation's definition ------------------------------------
        Act::AModel if reset => save_settings(&|a| {
            a.provider.clear();
            a.model.clear();
        }),
        Act::AModel => {
            let providers = store.providers.get_untracked();
            if providers.is_empty() {
                ctx.send(Cmd::LoadCatalog {
                    preferred_bundle: None,
                    preferred_flow: None,
                });
                store.rail.update(|r| {
                    r.save = SaveState::Refused(
                        "the gateway's providers are still loading — try again in a moment.".into(),
                    )
                });
                return;
            }
            let mut labels = vec!["Gateway default".to_string()];
            labels.extend(providers.iter().map(|p| p.name.clone()));
            let ctx2 = ctx.clone();
            let binding2 = binding.clone();
            let settings2 = settings.clone();
            pick_then(
                "Model — provider",
                labels,
                0,
                Rc::new(move |ix| {
                    let Some((_, def, a)) = settings2.clone() else {
                        return;
                    };
                    if ix == 0 {
                        let mut a = a;
                        a.provider.clear();
                        a.model.clear();
                        save_revision(
                            store,
                            &ctx2,
                            &binding2,
                            rail::settings_changes(&def.target, &a),
                        );
                        return;
                    }
                    let Some(p) = providers.get(ix - 1).cloned() else {
                        return;
                    };
                    // Stage 2 (the model) opens after this picker returns.
                    let ctx3 = ctx2.clone();
                    let binding3 = binding2.clone();
                    abstracttui::reactive::after(std::time::Duration::from_millis(1), move || {
                        let labels = p.models.clone();
                        let def = def.clone();
                        let a = a.clone();
                        let p2 = p.clone();
                        let ctx4 = ctx3.clone();
                        let binding4 = binding3.clone();
                        open_picker(
                            cx,
                            &ctx3,
                            Picker {
                                title: format!(
                                    "Model — {} — ↑↓ · Enter chooses · Esc back",
                                    p.name
                                ),
                                labels: labels.clone(),
                                live: None,
                                start: 0,
                                size: modal_size(90, (labels.len() as i32 + 6).min(30)),
                                hint: None,
                                live_hint: None,
                                keys: Vec::new(),
                                on_mount: None,
                                on_selection: None,
                                on_choose: Box::new(move |ix| {
                                    if let Some(m) = p2.models.get(ix) {
                                        let mut a = a.clone();
                                        a.provider = p2.name.clone();
                                        a.model = m.clone();
                                        save_revision(
                                            store,
                                            &ctx4,
                                            &binding4,
                                            rail::settings_changes(&def.target, &a),
                                        );
                                    }
                                    reopen(cx, store, &ctx4, &binding4, Panel::Model);
                                }),
                                on_cancel: Some(Box::new({
                                    let ctx5 = ctx3.clone();
                                    let binding5 = binding3.clone();
                                    move || reopen(cx, store, &ctx5, &binding5, Panel::Model)
                                })),
                            },
                        );
                    });
                }),
            );
        }
        Act::AReasoning if reset => save_settings(&|a| a.reasoning.clear()),
        Act::AReasoning => {
            let Some((_, _, a)) = settings.clone() else {
                return;
            };
            let probe = store.reasoning_probe.get_untracked();
            let levels: Vec<String> = probe
                .filter(|p| p.provider == a.provider && p.model == a.model)
                .map(|p| p.levels)
                .unwrap_or_default();
            if levels.is_empty() && !a.provider.is_empty() {
                ctx.send(Cmd::ProbeModelReasoning {
                    provider: a.provider.clone(),
                    model: a.model.clone(),
                });
            }
            if levels.is_empty() {
                store.rail.update(|r| {
                    r.save = SaveState::Refused(if a.provider.is_empty() {
                        "Reasoning and MTP choices appear when the gateway can describe the selected model.".into()
                    } else {
                        "Checking the model's reasoning levels… try again in a moment.".into()
                    })
                });
                return;
            }
            let mut labels = vec!["Gateway default".to_string()];
            labels.extend(levels.iter().cloned());
            let ctx2 = ctx.clone();
            let binding2 = binding.clone();
            let settings2 = settings.clone();
            pick_then(
                "Reasoning effort",
                labels,
                0,
                Rc::new(move |ix| {
                    let Some((_, def, mut a)) = settings2.clone() else {
                        return;
                    };
                    a.reasoning = if ix == 0 {
                        String::new()
                    } else {
                        levels.get(ix - 1).cloned().unwrap_or_default()
                    };
                    save_revision(
                        store,
                        &ctx2,
                        &binding2,
                        rail::settings_changes(&def.target, &a),
                    );
                }),
            );
        }
        Act::AMtp if reset => save_settings(&|a| a.speculation = None),
        Act::AMtp => {
            let Some((_, _, a)) = settings.clone() else {
                return;
            };
            let probe = store.execution_probe.get_untracked();
            let depths: Option<Vec<u64>> = probe
                .filter(|(p, m, _)| *p == a.provider && *m == a.model)
                .map(|(_, _, v)| {
                    v.pointer("/execution/speculation/supported_depths")
                        .and_then(Value::as_array)
                        .map(|d| d.iter().filter_map(Value::as_u64).collect())
                        .unwrap_or_default()
                });
            let Some(depths) = depths else {
                if !a.provider.is_empty() {
                    ctx.send(Cmd::ProbeModelExecution {
                        provider: a.provider.clone(),
                        model: a.model.clone(),
                    });
                }
                store.rail.update(|r| {
                    r.save = SaveState::Refused(if a.provider.is_empty() {
                        "Reasoning and MTP choices appear when the gateway can describe the selected model.".into()
                    } else {
                        "Checking MTP capability…".into()
                    })
                });
                return;
            };
            if depths.is_empty() {
                store.rail.update(|r| {
                    r.save =
                        SaveState::Refused("MTP is not supported on this model/backend.".into())
                });
                return;
            }
            let mut labels = vec!["Gateway default".to_string(), "Off".to_string()];
            labels.extend(depths.iter().map(|d| format!("Depth {d}")));
            let ctx2 = ctx.clone();
            let binding2 = binding.clone();
            let settings2 = settings.clone();
            pick_then(
                "MTP depth",
                labels,
                0,
                Rc::new(move |ix| {
                    let Some((_, def, mut a)) = settings2.clone() else {
                        return;
                    };
                    a.speculation = match ix {
                        0 => None,
                        1 => Some(json!({"enabled": false})),
                        n => depths
                            .get(n - 2)
                            .map(|d| json!({"enabled": true, "depth": d})),
                    };
                    save_revision(
                        store,
                        &ctx2,
                        &binding2,
                        rail::settings_changes(&def.target, &a),
                    );
                }),
            );
        }
        Act::AIterations | Act::ATokens | Act::AInstructions if reset => {
            save_settings(&|a| match act {
                Act::AIterations => a.max_iterations.clear(),
                Act::ATokens => a.max_tokens.clear(),
                _ => a.system.clear(),
            })
        }
        Act::AIterations | Act::ATokens | Act::AInstructions => {
            let Some((_, _, a)) = settings.clone() else {
                return;
            };
            let (title, info, initial) = match act {
                Act::AIterations => (
                    "Iteration limit",
                    "A whole number of iterations; empty = the workflow default.",
                    a.max_iterations.clone(),
                ),
                Act::ATokens => (
                    "Context token limit",
                    "The context window in tokens; empty = the gateway default.",
                    a.max_tokens.clone(),
                ),
                _ => (
                    "Additional instructions",
                    "Project conventions or guidance for every run; empty = none.",
                    a.system.clone(),
                ),
            };
            let ctx2 = ctx.clone();
            let binding2 = binding.clone();
            let settings2 = settings.clone();
            let which = act.clone();
            text_then(
                title,
                info,
                initial,
                Rc::new(move |v: String| {
                    let Some((_, def, mut a)) = settings2.clone() else {
                        return;
                    };
                    match which {
                        Act::AIterations => a.max_iterations = v.trim().to_string(),
                        Act::ATokens => a.max_tokens = v.trim().to_string(),
                        _ => a.system = v,
                    }
                    save_revision(
                        store,
                        &ctx2,
                        &binding2,
                        rail::settings_changes(&def.target, &a),
                    );
                }),
            );
        }
        Act::AWorkflow => {
            let workflows = s.workflows.clone();
            if workflows.is_empty() {
                ctx.send(Cmd::LoadCatalog {
                    preferred_bundle: None,
                    preferred_flow: None,
                });
                store.rail.update(|r| {
                    r.save = SaveState::Refused(
                        "the gateway's workflows are still loading — try again in a moment.".into(),
                    )
                });
                return;
            }
            let labels: Vec<String> = workflows.iter().map(|w| w.versioned_label()).collect();
            let ctx2 = ctx.clone();
            let binding2 = binding.clone();
            let settings2 = settings.clone();
            pick_then(
                "Workflow",
                labels,
                0,
                Rc::new(move |ix| {
                    let (Some(w), Some((_, def, _))) = (workflows.get(ix), settings2.clone())
                    else {
                        return;
                    };
                    let bundle_ref = if w.version.is_empty() {
                        w.bundle_id.clone()
                    } else {
                        format!("{}@{}", w.bundle_id, w.version)
                    };
                    save_revision(
                        store,
                        &ctx2,
                        &binding2,
                        Some(rail::workflow_changes(&def.target, &bundle_ref, &w.flow_id)),
                    );
                }),
            );
        }
        Act::ATitle | Act::AEvery => {
            let Some((summary, _, _)) = settings.clone() else {
                return;
            };
            let form = auto::revise_form_from(&summary);
            let email_trigger = crate::automation_email::is_email_trigger(
                &summary.trigger.source_id,
                summary.trigger.source_version,
            );
            let (title, info, initial) = if act == Act::ATitle {
                ("Title", "At most 120 characters.", form.title.clone())
            } else if email_trigger {
                (
                    crate::automation_email::EVERY_LABEL,
                    "A whole number of minutes, hours or days (e.g. 30m, 8h, 7d). The shortest interval is 60 s.",
                    form.every.clone().unwrap_or_default(),
                )
            } else {
                (
                    "Repeat every (UTC)",
                    "A whole number of minutes, hours or days (e.g. 30m, 8h, 7d).",
                    form.every.clone().unwrap_or_default(),
                )
            };
            let ctx2 = ctx.clone();
            let binding2 = binding.clone();
            let is_title = act == Act::ATitle;
            text_then(
                title,
                info,
                initial,
                Rc::new(move |v: String| {
                    let mut f = auto::revise_form_from(&summary);
                    if is_title {
                        f.title = v;
                    } else {
                        f.every = Some(v);
                    }
                    match auto::revise_changes(&summary, &f) {
                        Ok(ch) => save_revision(store, &ctx2, &binding2, ch),
                        Err(errors) => store
                            .rail
                            .update(|r| r.save = SaveState::Refused(errors.join(" "))),
                    }
                }),
            );
        }
        Act::ACount | Act::AUntil => {
            let Some((summary, def, _)) = settings.clone() else {
                return;
            };
            let form = auto::revise_form_with(&summary, Some(&def));
            let is_count = act == Act::ACount;
            let (title, info, initial) = if is_count {
                (
                    "Stop after this many runs",
                    "A whole number of at least 1; empty = no limit.",
                    form.count.clone().unwrap_or_default(),
                )
            } else {
                (
                    "Stop at (UTC)",
                    "A date and time read as UTC: YYYY-MM-DD HH:MM; empty = no end.",
                    form.until.clone().unwrap_or_default(),
                )
            };
            let ctx2 = ctx.clone();
            let binding2 = binding.clone();
            text_then(
                title,
                info,
                initial,
                Rc::new(move |v: String| {
                    let mut f = auto::revise_form_with(&summary, Some(&def));
                    if is_count {
                        f.count = Some(v.trim().to_string());
                    } else {
                        f.until = Some(v.trim().to_string());
                    }
                    match auto::revise_changes_with(&summary, Some(&def), &f) {
                        Ok(ch) => save_revision(store, &ctx2, &binding2, ch),
                        Err(errors) => store
                            .rail
                            .update(|r| r.save = SaveState::Refused(errors.join(" "))),
                    }
                }),
            );
        }
        Act::ANotify => {
            let Some((summary, def, _)) = settings.clone() else {
                return;
            };
            if !crate::automation_email::EmailStatus::usable(s.email.as_ref()) {
                store
                    .rail
                    .update(|r| r.save = SaveState::Refused("Connect a mailbox first.".into()));
                return;
            }
            let mut f = auto::revise_form_with(&summary, Some(&def));
            f.notify_email = Some(!f.notify_email.unwrap_or(false));
            match auto::revise_changes_with(&summary, Some(&def), &f) {
                Ok(ch) => save_revision(store, ctx, binding, ch),
                Err(errors) => store
                    .rail
                    .update(|r| r.save = SaveState::Refused(errors.join(" "))),
            }
        }
        Act::ARecipients => {
            use crate::automation_email as email;
            let Some((summary, def, _)) = settings.clone() else {
                return;
            };
            let form = auto::revise_form_with(&summary, Some(&def));
            let list = form.recipients.clone().unwrap_or_default();
            let ctx2 = ctx.clone();
            let binding2 = binding.clone();
            let save = Rc::new(move |recipients: email::RecipientsForm| {
                let mut f = auto::revise_form_with(&summary, Some(&def));
                f.notify_email = Some(true);
                f.recipients = Some(recipients);
                match auto::revise_changes_with(&summary, Some(&def), &f) {
                    Ok(ch) => save_revision(store, &ctx2, &binding2, ch),
                    Err(errors) => store
                        .rail
                        .update(|r| r.save = SaveState::Refused(errors.join(" "))),
                }
            });
            let ctx3 = ctx.clone();
            let binding3 = binding.clone();
            let ctx4 = ctx.clone();
            let binding4 = binding.clone();
            open_picker(
                cx,
                ctx,
                Picker {
                    title: format!(
                        "{} — ↑↓ · Enter chooses · Esc back",
                        email::RECIPIENTS_LEGEND
                    ),
                    labels: vec![
                        email::RECIPIENTS_SELF.to_string(),
                        email::RECIPIENTS_LIST.to_string(),
                    ],
                    live: None,
                    start: usize::from(list.list),
                    size: modal_size(90, 8),
                    hint: Some(email::RECIPIENTS_HINT.to_string()),
                    live_hint: None,
                    keys: Vec::new(),
                    on_mount: None,
                    on_selection: None,
                    on_choose: Box::new(move |ix| {
                        let back: Rc<dyn Fn()> = {
                            let ctx = ctx3.clone();
                            let binding = binding3.clone();
                            Rc::new(move || reopen(cx, store, &ctx, &binding, panel))
                        };
                        if ix == 0 {
                            save(email::RecipientsForm::default());
                            back();
                            return;
                        }
                        // "Me and these addresses": the addresses, then one revision.
                        let save = save.clone();
                        let back2 = back.clone();
                        crate::ui::automations_view::open_text(
                            cx,
                            &ctx3,
                            email::RECIPIENTS_LIST.to_string(),
                            vec![email::RECIPIENTS_HINT.to_string()],
                            list.addresses.clone(),
                            Rc::new(move |v: String| {
                                save(email::RecipientsForm {
                                    list: true,
                                    addresses: v,
                                });
                                back2();
                            }),
                            back,
                        );
                    }),
                    on_cancel: Some(Box::new(move || reopen(cx, store, &ctx4, &binding4, panel))),
                },
            );
        }
        Act::ATask => {
            let Some((_, def, _)) = settings.clone() else {
                return;
            };
            let before = def
                .target
                .pointer("/input_data/prompt")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let ctx2 = ctx.clone();
            let binding2 = binding.clone();
            text_then(
                "Task",
                "What every run is asked to do.",
                before.clone(),
                Rc::new(move |v: String| {
                    let prompt = v.trim().to_string();
                    if prompt.is_empty() {
                        store
                            .rail
                            .update(|r| r.save = SaveState::Refused("Task is required.".into()));
                        return;
                    }
                    if prompt == before.trim() {
                        store
                            .rail
                            .update(|r| r.save = SaveState::Refused("Nothing changed.".into()));
                        return;
                    }
                    let mut input = def.target.get("input_data").cloned().unwrap_or(json!({}));
                    input["prompt"] = json!(prompt);
                    save_revision(
                        store,
                        &ctx2,
                        &binding2,
                        Some(json!({"target": {
                            "bundle_ref": def.target.get("bundle_ref").cloned().unwrap_or(Value::Null),
                            "flow_id": def.target.get("flow_id").cloned().unwrap_or(Value::Null),
                            "input_data": input,
                        }})),
                    );
                }),
            );
        }
        Act::AContextMode => {
            let Some((summary, _, _)) = settings.clone() else {
                return;
            };
            let mut f = auto::revise_form_from(&summary);
            f.context = if summary.context_mode == "growing" {
                "independent".into()
            } else {
                "growing".into()
            };
            match auto::revise_changes(&summary, &f) {
                Ok(ch) => save_revision(store, ctx, binding, ch),
                Err(errors) => store
                    .rail
                    .update(|r| r.save = SaveState::Refused(errors.join(" "))),
            }
        }
        Act::AApproval => {
            let Some((_, def, _)) = settings.clone() else {
                return;
            };
            let next = if def.tool_approval == "ask" {
                "auto"
            } else {
                "ask"
            };
            save_revision(
                store,
                ctx,
                binding,
                Some(json!({"policy": {"tool_approval": next}})),
            );
        }
        Act::AToolsMode => {
            let available: Vec<String> = s
                .tools
                .iter()
                .filter(|t| !t.served_disabled)
                .map(|t| t.name.clone())
                .collect();
            save_settings(&|a| {
                a.tools = if a.tools.is_some() {
                    None
                } else {
                    Some(available.clone())
                };
            });
        }
        Act::AToggleTool(name) => {
            let custom = settings.as_ref().is_some_and(|(_, _, a)| a.tools.is_some());
            if !custom {
                store.rail.update(|r| {
                    r.save = SaveState::Refused("Choose Custom to pick tools one by one.".into())
                });
                return;
            }
            save_settings(&|a| {
                let list = a.tools.get_or_insert_with(Vec::new);
                if let Some(p) = list.iter().position(|t| *t == name) {
                    list.remove(p);
                } else {
                    list.push(name.clone());
                }
            });
        }
        Act::AToggleSkill(name) => {
            if s.skills.iter().any(|k| k.name == name && k.blocked) {
                return;
            }
            save_settings(&|a| {
                if let Some(p) = a.skills.iter().position(|x| *x == name) {
                    a.skills.remove(p);
                } else {
                    a.skills.push(name.clone());
                }
            });
        }
    }
}

fn toggle_group(open: Signal<Vec<(String, bool)>>, key: &str, s: &Snap, panel: Panel) {
    // The default fold state: the newest group open.
    let (cards_open, _) = (open.get_untracked(), panel);
    let default_open = {
        let (_, acts) = panel_cards(Panel::Activity, s, &cards_open, auto::now_unix());
        let first_or_last = match &s.binding {
            Binding::Conversation => acts.last().cloned(),
            Binding::Automation(_) => acts.first().cloned(),
        };
        match first_or_last {
            Some(Act::Group(k)) => k == key,
            Some(Act::ARun(i, _)) => format!("r:{i}") == key,
            _ => false,
        }
    };
    open.update(|v| {
        let now = v
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, o)| *o)
            .unwrap_or(default_open);
        v.retain(|(k, _)| k != key);
        v.push((key.to_string(), !now));
    });
}

/// Long-lived rail effects (wired once from `ui::root`): the conversation
/// handover after an archive, and the return to the rail after one of the
/// conversation's own pickers closes.
pub fn wire_rail(cx: Scope, store: Store, ctx: UiCtx) {
    {
        let ctx = ctx.clone();
        cx.effect(move || {
            let Some((archived, next)) = store.rail.with(|r| r.handover.clone()) else {
                return;
            };
            store.rail.update(|r| r.handover = None);
            if archived != store.session_id.get_untracked() {
                return;
            }
            let was_open = ctx.modal_open();
            match next {
                Some(id) => crate::ui::switch_session(store, &ctx, &id),
                None => crate::ui::new_session(store, &ctx),
            }
            if was_open {
                // The board stays up, now marking the newly open one.
                store.rail.update(|r| r.board_error.clear());
            }
        });
    }
    cx.effect(move || {
        let _ = ctx.modal_epoch.get();
        if ctx.modal_open() {
            return;
        }
        let Some((binding, panel)) = store.rail.with_untracked(|r| r.return_to.clone()) else {
            return;
        };
        store.rail.update(|r| r.return_to = None);
        let ctx2 = ctx.clone();
        abstracttui::reactive::after(std::time::Duration::from_millis(1), move || {
            if !ctx2.modal_open() {
                open_rail(cx, store, &ctx2, binding.clone(), panel);
            }
        });
    });
}
