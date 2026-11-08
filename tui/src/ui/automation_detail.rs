//! One automation, readable (operator 2026-11-08): the parts of the
//! `/automations` detail screen, built from AbstractTUI's own primitives —
//! `Block` panels, the kit's semantic `Tone` inks, the engine `List` for the
//! runs and a `Scroll` over a `MarkdownView` for the selected run's output.
//!
//! The screen (wide terminals): the title row; the header panel (the Active
//! switch, the schedule with the last and next run, what runs now, what
//! waits) beside the Details panel (labelled rows); the runs table (#,
//! status in its colour, when, tick; the waits that need you on top) beside
//! the panel of the selected row (its output as wrapped Markdown, artifacts
//! folded, the task); the key hints in two rows. Narrow terminals stack the
//! panels and move the details to the end of the output panel — it
//! scrolls, so nothing is ever cut.
//!
//! Pure parts (targets, cells, lines, the output document) are test-pinned;
//! the builders only paint what they return.

use abstracttui::prelude::*;
use abstracttui::render::rich::{RichLine, RichText, Span};
use abstracttui::render::{Attrs, Style};
use abstracttui::text;
use abstracttui::widgets::{MarkdownView, Tone};

use crate::automations::{self as auto, Control, Occurrence, Summary};
use crate::ui::automations_view::Target;

// ---------------------------------------------------------------------------
// Inks
// ---------------------------------------------------------------------------

/// How a piece of text is inked: the plain and quiet inks, the title
/// (accent, bold), bold text, or one of the kit's semantic tones (the
/// `Badge` vocabulary: ok, warn, error, info, accent, muted).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    Text,
    Muted,
    Faint,
    Title,
    Bold,
    Tone(Tone),
}

/// One line of a panel: its parts, each with its ink (wrapped at draw).
pub type Line = Vec<(String, Ink)>;

fn style_of(t: &TokenSet, ink: Ink) -> Style {
    match ink {
        Ink::Text => Style::new().fg(t.text),
        Ink::Muted => Style::new().fg(t.text_muted),
        Ink::Faint => Style::new().fg(t.text_faint),
        Ink::Title => Style::new().fg(t.accent).attrs(Attrs::BOLD),
        Ink::Bold => Style::new().fg(t.text).attrs(Attrs::BOLD),
        Ink::Tone(tone) => Style::new().fg(match tone {
            Tone::Accent => t.accent,
            Tone::Ok => t.ok,
            Tone::Warn => t.warn,
            Tone::Error => t.error,
            Tone::Info => t.info,
            Tone::Muted => t.text_muted,
        }),
    }
}

fn rich(t: &TokenSet, lines: &[Line]) -> RichText {
    RichText::from_lines(
        lines
            .iter()
            .map(|l| {
                RichLine::from_spans(
                    l.iter()
                        .filter(|(s, _)| !s.is_empty())
                        .map(|(s, ink)| Span::new(s.clone(), style_of(t, *ink)))
                        .collect(),
                )
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// Targets (what the cursor walks)
// ---------------------------------------------------------------------------

/// What the cursor walks, in screen order: the waits that need you, the
/// loaded runs NEWEST FIRST, then "Load earlier runs" while the gateway has
/// older ones.
pub fn targets(d: &auto::Detail) -> Vec<Target> {
    let mut out: Vec<Target> = Vec::new();
    if let Some(s) = &d.summary {
        out.extend(s.attention.waits.iter().cloned().map(Target::Wait));
    }
    let mut runs: Vec<&Occurrence> = d.occurrences.iter().collect();
    runs.sort_by_key(|o| std::cmp::Reverse(o.index));
    out.extend(runs.iter().map(|o| Target::Run(o.index)));
    if d.next_cursor.is_some() {
        out.push(Target::Earlier);
    }
    out
}

/// Where the cursor rests before the user moves it (`usize::MAX`): on the
/// first wait that needs you, else on the newest run.
pub fn resolve_cursor(cursor: usize, targets: &[Target]) -> usize {
    if cursor != usize::MAX {
        return cursor.min(targets.len().saturating_sub(1));
    }
    targets
        .iter()
        .position(|t| matches!(t, Target::Wait(_)))
        .or_else(|| targets.iter().position(|t| matches!(t, Target::Run(_))))
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// The runs table
// ---------------------------------------------------------------------------

/// A run's status as a glyph + the gateway's word, in its tone: running
/// (info), completed (ok), failed (error), waiting / retrying (warn).
pub fn run_status(o: &Occurrence) -> (String, Tone) {
    if o.status != "failed" && (!o.waits.is_empty() || o.status == "waiting") {
        return ("⚠ waiting".into(), Tone::Warn);
    }
    match o.status.as_str() {
        "failed" => ("✕ failed".into(), Tone::Error),
        "completed" => ("✓ completed".into(), Tone::Ok),
        "running" => ("● running".into(), Tone::Info),
        "admitted" => ("● starting".into(), Tone::Info),
        "backoff" => ("↻ retrying".into(), Tone::Warn),
        other => (format!("· {other}"), Tone::Muted),
    }
}

/// "07 Nov 23:34" (UTC; the column says so) from the served `fired_at`.
pub fn when_cell(o: &Occurrence) -> String {
    auto::format_utc_day(&o.fired_at)
}

/// The tick of a scheduled run ("29" from "schedule: Every 24 hours (UTC),
/// tick 29"); another trigger reads as its source ("manual"). The full
/// trigger sentence is in the run's own panel.
pub fn tick_cell(o: &Occurrence) -> String {
    let s = o.trigger_summary.as_str();
    if let Some(i) = s.rfind(", tick ") {
        return s[i + ", tick ".len()..].trim().to_string();
    }
    s.split(':').next().unwrap_or("").trim().to_string()
}

/// Column widths of the runs table: #, Status, When (UTC), Tick.
pub const RUN_COLS: [usize; 4] = [4, 12, 13, 6];
/// The runs panel's width: the columns, their gaps, the scrollbar and the
/// panel border.
pub const RUNS_PANEL_W: i32 = 4 + 1 + 12 + 1 + 13 + 1 + 6 + 1 + 2;

fn pad(s: &str, w: usize) -> String {
    let have = text::width(s).max(0) as usize;
    format!("{s}{}", " ".repeat(w.saturating_sub(have)))
}

/// The header row of the runs table.
pub fn runs_header() -> String {
    format!(
        "{} {} {} Tick",
        pad("#", RUN_COLS[0]),
        pad("Status", RUN_COLS[1]),
        pad("When (UTC)", RUN_COLS[2]),
    )
}

/// One row of the runs table per target: (its cells as one line, the
/// status part's tone). A wait spans the row; "Load earlier" is quiet.
pub fn target_row(d: &auto::Detail, target: &Target) -> Line {
    match target {
        Target::Wait(w) => {
            let run = w.index.map(|i| format!(" · run #{i}")).unwrap_or_default();
            vec![(
                format!("⚠ {}{run}", auto::wait_kind_label(&w.kind)),
                Ink::Tone(Tone::Warn),
            )]
        }
        Target::Run(i) => match d.occurrences.iter().find(|o| o.index == *i) {
            Some(o) => {
                let (status, tone) = run_status(o);
                vec![
                    (
                        format!("{} ", pad(&o.index.to_string(), RUN_COLS[0])),
                        Ink::Text,
                    ),
                    (pad(&status, RUN_COLS[1]), Ink::Tone(tone)),
                    (format!(" {}", pad(&when_cell(o), RUN_COLS[2])), Ink::Muted),
                    (format!(" {}", tick_cell(o)), Ink::Muted),
                ]
            }
            None => vec![(i.to_string(), Ink::Text)],
        },
        Target::Earlier => vec![("↓ Load earlier runs (Enter)".into(), Ink::Muted)],
    }
}

/// The runs panel's title: "Runs · 38", or "Runs · 50 of 141 shown" while
/// older pages are not loaded.
pub fn runs_title(d: &auto::Detail) -> String {
    let total = d
        .summary
        .as_ref()
        .map(|s| s.occurrence_count)
        .unwrap_or(0)
        .max(d.occurrences.len() as u64);
    let loaded = d.occurrences.len() as u64;
    if loaded < total {
        format!("Runs · {loaded} of {total} shown")
    } else {
        format!("Runs · {total}")
    }
}

// ---------------------------------------------------------------------------
// The header and Details panels
// ---------------------------------------------------------------------------

/// The header panel: the Active switch (+ what it means now), the
/// schedule with the last and next run, the served next run, what runs
/// now and what waits for you, a failed "Email result", the unseen
/// notices, and the archived/legacy notes.
pub fn header_lines(s: &Summary, busy: bool, pending: Option<Control>, now: i64) -> Vec<Line> {
    let row = crate::ui::automations_view::active_row(s, busy, pending);
    let mark = row.checked.unwrap_or(crate::ui::modals::Mark::Unavailable);
    let switch_ink = match mark {
        crate::ui::modals::Mark::On => Ink::Title,
        crate::ui::modals::Mark::Pending => Ink::Tone(Tone::Accent),
        crate::ui::modals::Mark::Unavailable => Ink::Faint,
        _ => Ink::Bold,
    };
    let (label, detail) = match row.text.split_once(" — ") {
        Some((l, d)) => (l.to_string(), format!(" — {d}")),
        None => (row.text.clone(), String::new()),
    };
    let mut out: Vec<Line> = vec![vec![
        (format!("{}{label}", mark.marker()), switch_ink),
        (detail, Ink::Muted),
    ]];
    out.push(vec![(auto::timing_line(s, now), Ink::Text)]);
    if let Some(next) = auto::run_now_next_line(s) {
        out.push(vec![(next, Ink::Muted)]);
    }
    let mut now_line: Line = Vec::new();
    if let Some(cur) = auto::current_label(s) {
        now_line.push((format!("● {cur}"), Ink::Tone(Tone::Info)));
    }
    if let Some(badge) = auto::waiting_badge(s) {
        if !now_line.is_empty() {
            now_line.push((" · ".into(), Ink::Faint));
        }
        now_line.push((format!("⚠ {badge}"), Ink::Tone(Tone::Warn)));
    }
    if !now_line.is_empty() {
        out.push(now_line);
    }
    if let Some(failed) = auto::email_failed_line(s, now) {
        out.push(vec![(failed, Ink::Tone(Tone::Error))]);
    }
    if !s.attention.items.is_empty() {
        let items: Vec<String> = s
            .attention
            .items
            .iter()
            .map(|i| {
                let kind = if i.kind == "failure" {
                    "failed"
                } else {
                    "notified"
                };
                format!("run #{} {kind}", i.index)
            })
            .collect();
        out.push(vec![(
            format!(
                "{} unseen: {}",
                s.attention.unseen_count.max(items.len() as u64),
                items.join(", ")
            ),
            Ink::Tone(Tone::Warn),
        )]);
    }
    if s.status == "archived" {
        out.push(vec![(
            "Archived: it will not run again; its history stays readable (Discuss still works)."
                .into(),
            Ink::Faint,
        )]);
    }
    if s.legacy {
        out.push(vec![(
            "Legacy schedule: managed with its existing controls (Observer run view).".into(),
            Ink::Faint,
        )]);
    }
    out
}

/// The Details panel as labelled rows: runs and revision, the context
/// mode (+ the growing settings as the gateway states them), the tool
/// policy, the workflow, the workspace folder (`w` browses it).
pub fn facts(s: &Summary, def: Option<&auto::Definition>) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    let mut runs = s.occurrence_count.to_string();
    if let Some(rev) = def.map(|d| d.revision).or(s.revision) {
        runs.push_str(&format!(" · revision {rev}"));
    }
    out.push(("Runs", runs));
    let mut context = auto::context_label(&s.context_mode).to_string();
    if let Some(d) = def {
        if s.context_mode == "growing" && !d.growing.is_empty() {
            let parts: Vec<String> = d
                .growing
                .iter()
                .map(|(k, v)| match v {
                    serde_json::Value::String(x) => format!("{k} {x}"),
                    other => format!("{k} {other}"),
                })
                .collect();
            context.push_str(&format!(" · {}", parts.join(" · ")));
        }
    }
    out.push(("Context", context));
    if let Some(d) = def {
        out.push((
            "Tools",
            if d.tool_approval == "ask" {
                "Ask me before each tool call".into()
            } else {
                "Run without asking".into()
            },
        ));
        if !d.workflow_id.is_empty() {
            out.push(("Workflow", d.workflow_id.clone()));
        }
    }
    if let Some(root) = &s.workspace_root {
        out.push((
            "Workspace",
            format!("{} (w browses it)", crate::ui::rail_view::short_name(root)),
        ));
    }
    out
}

/// The widths of the header panel and the Details panel side by side in
/// `inner` cells (one cell between them): the Details get the larger share
/// (labelled rows with long values).
pub fn top_widths(inner: i32) -> (i32, i32) {
    let head = (inner - 1) * 9 / 20;
    (head, inner - 1 - head)
}

/// Label column of the Details rows (the longest label + two cells).
pub const FACT_LABEL_W: usize = 11;

// ---------------------------------------------------------------------------
// The selected row's panel
// ---------------------------------------------------------------------------

/// What the panel beside the runs shows for the selected row: its title,
/// the inked lines above the text (status, when, trigger, a failure, a
/// notice), and the Markdown it scrolls through.
#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub title: String,
    pub meta: Vec<Line>,
    pub doc: String,
}

/// The engine's Markdown reader starts a list only after a blank line
/// (its documented subset); a run's text often puts the list right under
/// its lead-in line ("used for:\n- HDD 12To\n- HDD 16To"), which then read
/// as one paragraph. A list item that follows a text line gets that blank
/// line, so bullets stay bullets (CommonMark's own rule: a list may
/// interrupt a paragraph). Fenced code is left untouched.
pub fn lists_after_blank_lines(text: &str) -> String {
    fn is_item(t: &str) -> bool {
        if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
            return true;
        }
        let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
        digits > 0 && t[digits..].starts_with(". ")
    }
    let mut out = String::with_capacity(text.len() + 16);
    let mut in_fence = false;
    let mut prev_text = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("```") {
            in_fence = !in_fence;
            prev_text = false;
        } else if !in_fence {
            let item = is_item(t);
            if item && prev_text {
                out.push('\n');
            }
            prev_text = !item && !t.is_empty();
        }
        out.push_str(line);
        out.push('\n');
    }
    if !text.ends_with('\n') {
        out.pop();
    }
    out
}

/// Backslash-escape what the Markdown reader would otherwise read as
/// markup in a value we insert (never in the run's own answer).
pub fn md_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '*' | '~' | '`' | '[' | ']' | '(' | ')' | '\\' | '#' | '|' | '_'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The selected row's panel. `artifacts_open`: the artifact names are
/// listed (`l`), else folded to "N artifacts". `details`: the Details rows
/// close the document (narrow terminals, where the panel beside the header
/// does not fit).
pub fn output(
    d: &auto::Detail,
    at: Option<&Target>,
    artifacts_open: bool,
    details: Option<&[(&'static str, String)]>,
) -> Output {
    let mut out = match at {
        Some(Target::Run(i)) => match d.occurrences.iter().find(|o| o.index == *i) {
            Some(o) => run_output(d, o, artifacts_open),
            None => Output {
                title: format!("Run #{i}"),
                meta: Vec::new(),
                doc: "*This run is not loaded.*".into(),
            },
        },
        Some(Target::Wait(w)) => wait_output(w),
        Some(Target::Earlier) => Output {
            title: "Earlier runs".into(),
            meta: Vec::new(),
            doc: "The gateway keeps older runs. **Enter** loads the next page of them.".into(),
        },
        None if d.summary.is_none() => Output {
            title: "Runs".into(),
            meta: Vec::new(),
            doc: "*Loading automation…*".into(),
        },
        None => Output {
            title: "Runs".into(),
            meta: Vec::new(),
            doc: "No runs yet — **g** runs it now.".into(),
        },
    };
    if let Some(rows) = details {
        out.doc.push_str("\n\n---\n\n**Details**\n\n");
        let items: Vec<String> = rows
            .iter()
            .map(|(k, v)| format!("- **{k}:** {}", md_escape(v)))
            .collect();
        out.doc.push_str(&items.join("\n"));
    }
    out
}

fn run_output(d: &auto::Detail, o: &Occurrence, artifacts_open: bool) -> Output {
    let (status, tone) = run_status(o);
    let mut first: Line = vec![(status, Ink::Tone(tone))];
    let mut facts = Vec::new();
    if !o.fired_at.is_empty() {
        facts.push(auto::format_utc(&o.fired_at));
    }
    if !o.trigger_summary.is_empty() {
        facts.push(o.trigger_summary.clone());
    }
    if o.attempts > 1 {
        facts.push(format!("{} attempts", o.attempts));
    }
    if !facts.is_empty() {
        first.push((format!(" · {}", facts.join(" · ")), Ink::Muted));
    }
    let mut meta = vec![first];
    if let Some(f) = &o.failure {
        meta.push(vec![
            ("Failed: ".into(), Ink::Tone(Tone::Error)),
            (
                format!(
                    "{} {} (after {} attempts)",
                    f.reason_code, f.message, f.attempts
                ),
                Ink::Tone(Tone::Error),
            ),
        ]);
    }
    if let Some((title, body)) = &o.notify {
        meta.push(vec![
            ("Notified: ".into(), Ink::Tone(Tone::Accent)),
            (format!("{title} — {body}"), Ink::Text),
        ]);
    }
    // An unseen notice of this run the lines above do not already say.
    if let Some(s) = &d.summary {
        for item in s.attention.items.iter().filter(|i| i.index == o.index) {
            let said = (item.kind == "failure" && o.failure.is_some())
                || (item.kind != "failure" && o.notify.is_some());
            if !said {
                meta.push(vec![
                    ("New: ".into(), Ink::Tone(Tone::Warn)),
                    (format!("{} — {}", item.title, item.body), Ink::Text),
                ]);
            }
        }
    }
    let mut doc = if !o.answer.trim().is_empty() {
        lists_after_blank_lines(&o.answer)
    } else if matches!(
        o.status.as_str(),
        "running" | "waiting" | "backoff" | "admitted"
    ) {
        "*No output yet: the run is in progress.*".to_string()
    } else if o.status == "failed" {
        "*No output: the run failed.*".to_string()
    } else {
        "*No output.*".to_string()
    };
    if !o.artifacts.is_empty() {
        let n = o.artifacts.len();
        let noun = if n == 1 { "artifact" } else { "artifacts" };
        doc.push_str("\n\n---\n\n");
        if artifacts_open {
            doc.push_str(&format!("**{n} {noun}** — **l** folds them\n\n"));
            let names: Vec<String> = o
                .artifacts
                .iter()
                .map(|a| format!("- `{}`", a.replace('`', "'")))
                .collect();
            doc.push_str(&names.join("\n"));
        } else {
            doc.push_str(&format!("**{n} {noun}** — **l** lists them"));
        }
    }
    if !o.user_turn.trim().is_empty() {
        doc.push_str("\n\n---\n\n**Task**\n\n");
        doc.push_str(&lists_after_blank_lines(o.user_turn.trim()));
    }
    Output {
        title: format!("Run #{}", o.index),
        meta,
        doc,
    }
}

fn wait_output(w: &auto::Wait) -> Output {
    let run = w.index.map(|i| format!(" · run #{i}")).unwrap_or_default();
    let keys = match w.kind.as_str() {
        "tool_approval" => "y approves · n denies",
        "ask_user" | "event" => "Enter answers",
        _ => "not answered here (open its run)",
    };
    let mut doc = String::new();
    if !w.prompt.is_empty() {
        doc.push_str(&w.prompt);
    }
    if !w.choices.is_empty() {
        doc.push_str("\n\n**Choices**\n\n");
        let items: Vec<String> = w
            .choices
            .iter()
            .map(|c| format!("- {}", md_escape(c)))
            .collect();
        doc.push_str(&items.join("\n"));
    }
    let calls = auto::wait_tool_calls(w);
    if !calls.is_empty() {
        doc.push_str("\n\n**Tool calls**\n\n");
        let items: Vec<String> = calls
            .iter()
            .map(|c| format!("- `{}`", c.replace('`', "'")))
            .collect();
        doc.push_str(&items.join("\n"));
    }
    if doc.is_empty() {
        doc.push_str("*Nothing more was said.*");
    }
    Output {
        title: format!("{}{run}", auto::wait_kind_label(&w.kind)),
        meta: vec![vec![(format!("⚠ {keys}"), Ink::Tone(Tone::Warn))]],
        doc: doc.trim_start().to_string(),
    }
}

// ---------------------------------------------------------------------------
// Key hints: two rows, the automation's then the selected row's
// ---------------------------------------------------------------------------

/// The automation's keys (the header's buttons).
pub const AUTOMATION_HINTS: &[(&str, &str)] = &[
    ("g", "Run now"),
    ("x", "Stop"),
    ("e", "Edit"),
    ("a", "Archive"),
    ("u", "Unarchive"),
    ("space", "Active"),
    ("o", "Chat"),
    ("w", "Folder"),
    ("r", "Refresh"),
    ("Esc", "Back"),
];

/// The selected row's keys.
pub const SELECTION_HINTS: &[(&str, &str)] = &[
    ("↑↓", "Select"),
    ("PgUp/PgDn", "Scroll"),
    ("Enter", "Open"),
    ("d", "Discuss"),
    ("y/n", "Approve/deny"),
    ("l", "Artifacts"),
    ("Ctrl+P", "Read aloud"),
];

/// The hint groups as lines that fit `width`: whole pairs only (never an
/// ellipsis), each group on its own row(s); keys bold, labels muted.
pub fn hint_lines(groups: &[&[(&str, &str)]], width: usize) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    for group in groups {
        let mut cur: Line = Vec::new();
        let mut cur_w = 0usize;
        for (k, l) in group.iter() {
            let item_w = text::width(k).max(0) as usize + 1 + text::width(l).max(0) as usize;
            let sep = if cur.is_empty() { 0 } else { 3 };
            if !cur.is_empty() && cur_w + sep + item_w > width {
                out.push(std::mem::take(&mut cur));
                cur_w = 0;
            }
            if !cur.is_empty() {
                cur.push((" · ".into(), Ink::Faint));
                cur_w += 3;
            }
            cur.push((k.to_string(), Ink::Bold));
            cur.push((format!(" {l}"), Ink::Muted));
            cur_w += item_w;
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------

// The engine's solver sizes a row's cross axis from its children measured
// at the ROW's width, so text measured inside a narrower bordered panel
// came out short (and painted over the border). Every block of text here is
// therefore wrapped at the width it is drawn at — known when the screen
// opens — and given exactly its rows.

/// Inked lines wrapped at `width`: the view (exactly its rows) and its height.
pub fn lines_view(lines: &[Line], width: i32) -> (View, i32) {
    let t = abstracttui::app::current_theme().tokens;
    let doc = rich(&t, lines).wrap(width.max(1));
    let rows = doc.height();
    let view = Element::new()
        .style(LayoutStyle::line(rows).shrink(0.0))
        .draw(move |canvas, rect| {
            for (row, line) in doc.lines.iter().enumerate() {
                let y = rect.y + row as i32;
                if y >= rect.bottom() {
                    break;
                }
                let mut x = rect.x;
                for span in &line.spans {
                    canvas.print_styled(Point::new(x, y), &span.text, &span.style);
                    x += span.width();
                }
            }
        })
        .build();
    (view, rows)
}

/// The key hints wrapped at `width` (whole pairs, each group its own rows).
pub fn hints_view(groups: &[&[(&str, &str)]], width: i32) -> (View, i32) {
    lines_view(&hint_lines(groups, width.max(1) as usize), width)
}

/// The Details rows at `width`: a muted label column, the value wrapped
/// beside it (continuation rows keep the value's column).
pub fn facts_view(rows: &[(&'static str, String)], width: i32) -> (View, i32) {
    let t = abstracttui::app::current_theme().tokens;
    let value_w = (width - FACT_LABEL_W as i32).max(8);
    let mut laid: Vec<(String, String)> = Vec::new();
    for (label, value) in rows {
        for (i, line) in text::wrap(value, value_w).into_iter().enumerate() {
            let head = if i == 0 {
                label.to_string()
            } else {
                String::new()
            };
            laid.push((head, line));
        }
    }
    let n = laid.len() as i32;
    let view = Element::new()
        .style(LayoutStyle::line(n).shrink(0.0))
        .draw(move |canvas, rect| {
            for (row, (label, value)) in laid.iter().enumerate() {
                let y = rect.y + row as i32;
                if y >= rect.bottom() {
                    break;
                }
                canvas.print_styled(Point::new(rect.x, y), label, &style_of(&t, Ink::Muted));
                canvas.print_styled(
                    Point::new(rect.x + FACT_LABEL_W as i32, y),
                    value,
                    &style_of(&t, Ink::Text),
                );
            }
        })
        .build();
    (view, n)
}

/// A titled panel (the crate's `Block`, rounded) around `children`, with
/// one cell of air inside the border on each side ([`PANEL_CHROME`] cells
/// across in all).
pub fn panel(title: String, layout: LayoutStyle, children: Vec<View>) -> Element {
    let t = abstracttui::app::current_theme().tokens;
    let mut inner = Element::new().style(
        LayoutStyle::column()
            .gap(1)
            .grow(1.0)
            .basis(Dimension::Cells(0))
            .padding(Edges {
                left: 1,
                right: 1,
                top: 0,
                bottom: 0,
            }),
    );
    for c in children {
        inner = inner.child(c);
    }
    Block::new()
        .border(BorderKind::Rounded)
        .title(title)
        .layout(layout)
        .child(inner.build())
        .element(&t)
}

/// A phone-width or short terminal's frameless panel: its title as a
/// bold line, then `children` (one row of chrome instead of two, no
/// columns given to a border).
pub fn section(title: String, layout: LayoutStyle, children: Vec<View>) -> Element {
    let (head, _) = lines_view(&[vec![(title, Ink::Bold)]], 400);
    let mut el = Element::new().style(layout).child(
        Element::new()
            .style(LayoutStyle::line(1).shrink(0.0).clip())
            .child(head)
            .build(),
    );
    for c in children {
        el = el.child(c);
    }
    el
}

/// Cells a [`panel`] takes across: its border and the air inside it.
pub const PANEL_CHROME: i32 = 4;

/// The runs panel: the table edge to edge inside the border (the engine
/// list's own ground), no air.
pub fn table_panel(title: String, layout: LayoutStyle, children: Vec<View>) -> Element {
    let t = abstracttui::app::current_theme().tokens;
    let mut b = Block::new()
        .border(BorderKind::Rounded)
        .title(title)
        .layout(layout);
    for c in children {
        b = b.child(c);
    }
    b.element(&t)
}

/// The output's Markdown (the crate's reader: paragraphs, headings,
/// bullets, code), wrapped at the panel width.
pub fn markdown(doc: String) -> View {
    let t = abstracttui::app::current_theme().tokens;
    // Auto height: inside the output's Scroll the view answers its typeset
    // row count (its default grow/basis-0 would take leftover space only).
    MarkdownView::new(doc)
        .layout(LayoutStyle::default().shrink(0.0))
        .element(&t)
        .build()
}

/// The header row of the runs table, in the engine `Table`'s header
/// vocabulary (raised ground, muted bold titles).
pub fn runs_header_view() -> View {
    let t = abstracttui::app::current_theme().tokens;
    let header = runs_header();
    Element::new()
        .style(LayoutStyle::line(1).shrink(0.0))
        .draw(move |canvas, rect| {
            let style = Style::new()
                .fg(t.text_muted)
                .bg(t.surface_raised)
                .attrs(Attrs::BOLD);
            canvas.fill_styled(Rect::new(rect.x, rect.y, rect.w, 1), ' ', &style);
            canvas.print_styled(Point::new(rect.x, rect.y), &header, &style);
        })
        .build()
}

/// The runs table's rows as rich List items. The selected row wears the
/// selection colours whole; the others carry their status tone.
pub fn run_items(d: &auto::Detail, targets: &[Target], selected: usize) -> Vec<RichText> {
    let t = abstracttui::app::current_theme().tokens;
    targets
        .iter()
        .enumerate()
        .map(|(i, target)| {
            let line = target_row(d, target);
            if i == selected {
                RichText::from_lines(vec![RichLine::from_spans(
                    line.into_iter()
                        .map(|(s, _)| Span::new(s, Style::new().attrs(Attrs::BOLD)))
                        .collect(),
                )])
            } else {
                rich(&t, &[line])
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn occ(index: u64, status: &str) -> Occurrence {
        Occurrence {
            run_id: format!("run-{index}"),
            index,
            attempts: 1,
            fired_at: "2026-11-07T23:34:34+00:00".into(),
            status: status.into(),
            trigger_summary: format!("schedule: Every 24 hours (UTC), tick {}", index - 1),
            user_turn: "Check the prices.".into(),
            answer: String::new(),
            notify: None,
            failure: None,
            artifacts: Vec::new(),
            waits: Vec::new(),
        }
    }

    #[test]
    fn a_run_reads_as_status_in_its_tone_when_and_tick() {
        assert_eq!(run_status(&occ(1, "completed")).1, Tone::Ok);
        assert_eq!(run_status(&occ(1, "failed")).1, Tone::Error);
        assert_eq!(run_status(&occ(1, "running")).1, Tone::Info);
        assert_eq!(run_status(&occ(1, "waiting")).1, Tone::Warn);
        assert_eq!(when_cell(&occ(1, "completed")), "07 Nov 23:34");
        assert_eq!(tick_cell(&occ(30, "completed")), "29");
        let mut manual = occ(2, "completed");
        manual.trigger_summary = "manual: run now (dadd916a)".into();
        assert_eq!(tick_cell(&manual), "manual");
    }

    #[test]
    fn the_runs_are_listed_newest_first_after_the_waits_and_the_cursor_rests_on_the_newest() {
        let d = auto::Detail {
            id: "a".into(),
            occurrences: vec![occ(1, "completed"), occ(2, "completed"), occ(3, "running")],
            next_cursor: Some("c".into()),
            ..Default::default()
        };
        let ts = targets(&d);
        assert_eq!(
            ts,
            vec![
                Target::Run(3),
                Target::Run(2),
                Target::Run(1),
                Target::Earlier
            ]
        );
        assert_eq!(resolve_cursor(usize::MAX, &ts), 0);
        assert_eq!(resolve_cursor(9, &ts), 3);
    }

    #[test]
    fn artifacts_fold_to_a_count_and_list_on_l() {
        let mut o = occ(4, "completed");
        o.answer = "First paragraph.\n\n- a bullet\n- another".into();
        o.artifacts = vec!["25e0741f00b20e82cc896b7bfc0ede67".into(); 5];
        let d = auto::Detail {
            id: "a".into(),
            occurrences: vec![o],
            ..Default::default()
        };
        let folded = output(&d, Some(&Target::Run(4)), false, None);
        assert!(folded.doc.contains("**5 artifacts** — **l** lists them"));
        assert!(!folded.doc.contains("25e0741f"), "{}", folded.doc);
        assert!(folded.doc.starts_with("First paragraph.\n\n- a bullet"));
        assert!(
            folded.doc.ends_with("**Task**\n\nCheck the prices."),
            "{}",
            folded.doc
        );
        let open = output(&d, Some(&Target::Run(4)), true, None);
        assert_eq!(
            open.doc.matches("25e0741f00b20e82cc896b7bfc0ede67").count(),
            5
        );
    }

    #[test]
    fn a_list_right_under_its_lead_in_line_stays_a_list() {
        assert_eq!(
            lists_after_blank_lines("used for:\n- HDD 12To\n- HDD 16To\nThen 1 more."),
            "used for:\n\n- HDD 12To\n- HDD 16To\nThen 1 more."
        );
        assert_eq!(
            lists_after_blank_lines("Steps:\n1. read\n2. write\n"),
            "Steps:\n\n1. read\n2. write\n"
        );
        // Already separated, emphasis, and fenced code are left alone.
        let kept = "a\n\n- b\n*not a list*\n```\nx:\n- y\n```";
        assert_eq!(lists_after_blank_lines(kept), kept);
    }

    #[test]
    fn hints_wrap_by_whole_pairs_in_their_own_groups() {
        let wide = hint_lines(&[AUTOMATION_HINTS, SELECTION_HINTS], 200);
        assert_eq!(wide.len(), 2);
        let narrow = hint_lines(&[AUTOMATION_HINTS, SELECTION_HINTS], 40);
        assert!(narrow.len() > 2);
        for line in &narrow {
            let w: i32 = line.iter().map(|(s, _)| text::width(s)).sum();
            assert!(w <= 40, "{line:?}");
        }
    }
}
