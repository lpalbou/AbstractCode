//! `/export` — transcript export: archival markdown + SFT/CPT JSONL.
//!
//! ## Spec (the decisions, made here)
//!
//! **Syntax**: `/export [md|markdown|jsonl] [--details] [path]`, tokens in
//! that order but each optional. Bare `/export` does the obviously-right
//! thing: markdown, auto-named `abstractcode-export-<sid8>-<stamp>.md` in
//! the current directory. The format word wins over the path's extension;
//! a CONFLICT between the two (`/export md out.jsonl`) refuses instead of
//! writing one format into the other's extension. A known extension alone
//! infers the format (`/export out.jsonl` = jsonl). `--details` is
//! recognized anywhere; any other `-`-leading token refuses loudly (a
//! typo'd flag must never silently become an output FILENAME). One path
//! token max — paths with spaces are not expressible in the composer's
//! whitespace grammar (v1 limit, documented); a file literally named `md`
//! needs `./md`.
//!
//! **Scope**: the CURRENT agent-lane transcript as held by the client
//! ([`Item`] list from the fold). v1 exports the agent lane only — entity
//! visits are separate server-side conversations. The export flag, not the
//! view toggle, decides detail level: a script calling `/export` gets
//! stable output regardless of Ctrl+D state.
//!
//! **Default view vs `--details`**: the default mirrors the clean view
//! (user prompts, assistant answers, steers, info/error lines) PLUS
//! one-line tool activity summaries — the live view folds finished-OK tool
//! cards away as a screen-space economy, but archival wants the activity
//! trace, and errored tools keep their full card in both modes (the view's
//! own honesty rule; "errored" is STATUS-based — `Failed`/`Denied` — never
//! the error string alone, which can be empty while the failure text
//! rides the result preview). `--details` adds thinking/reasoning blocks
//! and full tool cards (args + result previews).
//!
//! **Honest bounds**: the fold truncates old items ([`Fold::truncated`]) —
//! a truncated MARKDOWN export says so in its header line; JSONL is
//! schema-pure by design (no header line — strict trainability), so the
//! on-screen notice carries the warning there, naming the consequence:
//! the earliest turns are missing from every line's prefix. Never
//! pretend completeness. Bodies are exported exactly as held: prompts
//! and answers are full text; tool args/results are the fold's
//! preview-bounded copies (`[#TRUNCATION]` markers ride along where the
//! fold cut). No EXTRA truncation happens here. Images are referenced by
//! artifact id + label, never bytes.
//!
//! **JSONL (the SFT half)**: one JSON object per line, OpenAI chat schema
//! `{"messages":[{"role":"user",...},{"role":"assistant",...}]}` — ONE
//! LINE PER COMPLETED TURN, each carrying the conversation prefix up to
//! and including that turn's final answer. Rationale: every line is a
//! self-contained training example (SFT loaders consume it directly, no
//! cross-line stitching); the last line is the whole session, so
//! whole-session/CPT consumers take just the final line while SFT
//! consumers take every line (the standard multi-turn expansion). Pairing
//! semantics REUSE [`Fold::chat_messages`]' rule (pinned by a parity
//! test): a `final_answer: true` assistant item answers the newest open
//! user item; unanswered (failed/cancelled) turns are EXCLUDED — a
//! dangling user prompt is provider-hostile — and counted in the caller's
//! notice, never written to the file. Default lines carry ONLY the
//! `messages` key (drop-in trainable; strict validators reject unknown
//! keys). `--details` adds a `details` side field (that turn's tools,
//! cycles, steers) instead of fabricating assistant `tool_calls`: the
//! client holds preview-bounded STRINGS, not wire-faithful call
//! structures — minting tool_calls with truncated non-JSON arguments
//! would teach a model malformed calls. Faithful tool traces live in the
//! gateway run ledgers.
//!
//! **File write**: never overwrites (atomic `create_new`, no
//! check-then-write race), never creates parent directories, never
//! expands `~` (refused with a pointer). Success notices name the
//! absolute path + item/line counts.
//!
//! Pure renderers + one small fs helper; no UI imports (house rule:
//! 1 file = 1 task, logic separated from I/O orchestration).

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::transcript::{Item, ToolStatus};

pub const USAGE: &str = "usage: /export [md|jsonl] [--details] [path]";

// ---------------------------------------------------------------------------
// Argument parsing (token semantics; `commands::parse` keeps the raw rest)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Markdown,
    Jsonl,
}

impl ExportFormat {
    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Markdown => "markdown",
            ExportFormat::Jsonl => "jsonl",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Markdown => "md",
            ExportFormat::Jsonl => "jsonl",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportArgs {
    pub format: ExportFormat,
    pub details: bool,
    /// Explicit output path (verbatim); `None` = auto-named in the cwd.
    pub path: Option<String>,
}

/// Parse the raw rest of `/export`. See the module doc for the grammar.
pub fn parse_args(rest: &str) -> Result<ExportArgs, String> {
    let mut format_word: Option<ExportFormat> = None;
    let mut details = false;
    let mut path: Option<String> = None;
    for tok in rest.split_whitespace() {
        if tok == "--details" {
            details = true;
            continue;
        }
        if tok.starts_with('-') {
            // A typo'd flag must never silently become an output filename.
            return Err(format!("unknown flag {tok} — {USAGE}"));
        }
        // The format word is claimable only BEFORE a path token appeared:
        // `/export report.md md` is a second path (refused), not a format.
        if format_word.is_none() && path.is_none() {
            match tok.to_ascii_lowercase().as_str() {
                "md" | "markdown" => {
                    format_word = Some(ExportFormat::Markdown);
                    continue;
                }
                "jsonl" => {
                    format_word = Some(ExportFormat::Jsonl);
                    continue;
                }
                _ => {}
            }
        }
        if path.is_none() {
            path = Some(tok.to_string());
        } else {
            return Err(format!(
                "one output path expected — got both {} and {tok} ({USAGE}; paths with spaces are not supported)",
                path.as_deref().unwrap_or("")
            ));
        }
    }
    let inferred = path.as_deref().and_then(|p| {
        match Path::new(p)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref()
        {
            Some("jsonl") => Some(ExportFormat::Jsonl),
            Some("md") | Some("markdown") => Some(ExportFormat::Markdown),
            _ => None,
        }
    });
    let format = match (format_word, inferred) {
        // Explicit word vs known extension disagreeing: writing markdown
        // into `.jsonl` (or vice versa) poisons downstream loaders —
        // refuse rather than pick a winner silently.
        (Some(w), Some(i)) if w != i => {
            return Err(format!(
                "format {} conflicts with the .{} extension — drop one ({USAGE})",
                w.label(),
                i.extension()
            ));
        }
        (Some(w), _) => w,
        (None, Some(i)) => i,
        (None, None) => ExportFormat::Markdown,
    };
    Ok(ExportArgs {
        format,
        details,
        path,
    })
}

// ---------------------------------------------------------------------------
// Output path + file write
// ---------------------------------------------------------------------------

/// `abstractcode-export-<sid8>-<YYYYMMDD-HHMMSS>.<ext>` — the session id's
/// `acode-` prefix is stripped so the 8 chars carry entropy, not the brand.
pub fn default_filename(session_id: &str, now_iso: &str, format: ExportFormat) -> String {
    let bare = session_id.strip_prefix("acode-").unwrap_or(session_id);
    let short: String = bare.chars().take(8).collect();
    let short = if short.is_empty() {
        "session".to_string()
    } else {
        short
    };
    format!(
        "abstractcode-export-{short}-{}.{}",
        compact_timestamp(now_iso),
        format.extension()
    )
}

/// `2026-07-24T15:59:03Z` → `20260724-155903` (digit fold — tolerant of
/// any ISO-shaped input; a malformed stamp degrades to its raw digits
/// rather than panicking on slice bounds).
fn compact_timestamp(iso: &str) -> String {
    let digits: String = iso.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 14 {
        format!("{}-{}", &digits[..8], &digits[8..14])
    } else {
        digits
    }
}

/// Resolve the output path: explicit wins verbatim; `~` is REFUSED (the
/// shell expands tildes, not this client — silently writing a literal
/// `./~/x` file would be worse); default = auto-name relative to the cwd.
pub fn resolve_output_path(
    path_arg: Option<&str>,
    session_id: &str,
    now_iso: &str,
    format: ExportFormat,
) -> Result<PathBuf, String> {
    match path_arg {
        Some(p) if p.starts_with('~') => Err(
            "path starts with ~ — the shell's tilde is not expanded here; use an absolute or relative path"
                .to_string(),
        ),
        Some(p) => Ok(PathBuf::from(p)),
        None => Ok(PathBuf::from(default_filename(session_id, now_iso, format))),
    }
}

/// Write to a NEW file only. `create_new` makes the no-overwrite guarantee
/// atomic (no check-then-write race) and doubles as the parent-dir check:
/// a missing parent surfaces as NotFound — `/export` never creates
/// directory trees silently.
pub fn write_new_file(path: &Path, contents: &str) -> Result<(), String> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => format!(
                "already exists: {} — /export never overwrites (pass a new path)",
                path.display()
            ),
            std::io::ErrorKind::NotFound => format!(
                "parent directory does not exist for {} — /export never creates directories",
                path.display()
            ),
            _ => format!("cannot write {}: {e}", path.display()),
        })?;
    file.write_all(contents.as_bytes())
        .map_err(|e| format!("write failed for {}: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// Shared item predicates
// ---------------------------------------------------------------------------

/// True when the transcript holds any CONVERSATION at all — boot pushes
/// Info notices (session echo, workspace policy), and exporting a
/// notices-only fold would archive nothing worth keeping.
pub fn has_conversation(items: &[Item]) -> bool {
    items.iter().any(|i| !matches!(i, Item::Info { .. }))
}

/// Which items the chosen mode exports. Thinking/Probe are details-gated
/// (the view's rule); everything else exports in both modes — including
/// finished-OK tools, which the live view folds away as a SCREEN-SPACE
/// economy that archival deliberately does not inherit (one-line summary
/// in default mode, full card in details).
pub fn included(item: &Item, details: bool) -> bool {
    match item {
        Item::Thinking { .. } | Item::Probe { .. } => details,
        _ => true,
    }
}

#[derive(Debug, Clone, Default)]
pub struct ExportMeta {
    pub session_id: String,
    /// Workflow label ("" = unknown/never loaded; omitted from the header).
    pub workflow: String,
    /// ISO UTC timestamp of the export.
    pub exported_at: String,
    /// The fold dropped older items — the export must say so.
    pub truncated: bool,
}

fn status_word(s: ToolStatus) -> &'static str {
    match s {
        ToolStatus::AwaitingApproval => "awaiting_approval",
        ToolStatus::Running => "running",
        ToolStatus::Ok => "ok",
        ToolStatus::Failed => "failed",
        ToolStatus::Denied => "denied",
    }
}

fn status_glyph(s: ToolStatus) -> &'static str {
    // The view's vocabulary (docs/api.md "Transcript vocabulary").
    match s {
        ToolStatus::AwaitingApproval => "?",
        ToolStatus::Running => "»",
        ToolStatus::Ok => "✓",
        ToolStatus::Failed => "✗",
        ToolStatus::Denied => "⊘",
    }
}

// ---------------------------------------------------------------------------
// Markdown
// ---------------------------------------------------------------------------

/// A code fence longer than any backtick run in `body` (min 3): a tool
/// result containing ``` must not break out of its fenced block.
fn fence_for(body: &str) -> String {
    "`".repeat(longest_backtick_run(body).max(2) + 1)
}

/// Inline code span safe for backticks in the text (CommonMark: a longer
/// tick run + space padding when the content touches a backtick).
fn inline_code(text: &str) -> String {
    let run = longest_backtick_run(text);
    if run == 0 {
        return format!("`{text}`");
    }
    let ticks = "`".repeat(run + 1);
    format!("{ticks} {text} {ticks}")
}

fn longest_backtick_run(text: &str) -> usize {
    let mut longest = 0usize;
    let mut run = 0usize;
    for ch in text.chars() {
        if ch == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    longest
}

fn quoted(body: &str) -> String {
    let lines: Vec<String> = body
        .lines()
        .map(|l| {
            if l.is_empty() {
                ">".to_string()
            } else {
                format!("> {l}")
            }
        })
        .collect();
    if lines.is_empty() {
        ">".to_string()
    } else {
        lines.join("\n")
    }
}

fn is_wrapper_runtime_note(text: &str) -> bool {
    text == crate::runner::SUBRUN_CONCLUSION_NOTE
}

/// Render the transcript as a readable archival markdown document. Header
/// first (session, timestamp, workflow, counts, truncation honesty), then
/// the conversation with per-role `##` markers; intra-turn material
/// (tools, cycles, steers, errors) at `###`.
pub fn to_markdown(items: &[Item], meta: &ExportMeta, details: bool) -> String {
    let shown = items.iter().filter(|i| included(i, details)).count();
    let mut header = String::from("# AbstractCode transcript\n\n");
    header.push_str(&format!("- session: `{}`\n", meta.session_id));
    if !meta.workflow.is_empty() {
        header.push_str(&format!("- workflow: {}\n", meta.workflow));
    }
    header.push_str(&format!("- exported: {}\n", meta.exported_at));
    header.push_str(&format!(
        "- view: {}\n",
        if details {
            "full (--details: reasoning + tool cards)"
        } else {
            "clean (answers + tool activity)"
        }
    ));
    header.push_str(&format!(
        "- items: {shown} of {} held by the client\n",
        items.len()
    ));
    if meta.truncated {
        // Archival honesty with a CLIENT recovery path (operator ruling
        // 2026-07-26: user-facing text names actions, never ledgers).
        header.push_str(
            "- ⚠ INCOMPLETE: older items were dropped from the client view before export — reopen the session and scroll to the top to load them, then re-export\n",
        );
    }
    header.push_str(
        "\n> Bodies are exported as rendered in the TUI: prompts and answers are full\n\
         > text; tool args/results are the fold's preview-bounded copies. Agent-lane\n\
         > transcript only (v1).\n\n---",
    );

    let mut sections: Vec<String> = vec![header];
    for item in items {
        if !included(item, details) {
            continue;
        }
        match item {
            Item::User { text } => sections.push(format!("## User\n\n{text}")),
            Item::Assistant { text, final_answer } => {
                let heading = if *final_answer {
                    "## Assistant"
                } else {
                    "## Assistant (update)"
                };
                sections.push(format!("{heading}\n\n{text}"));
            }
            Item::Steer { text } => sections.push(format!("### ↪ Steer\n\n{text}")),
            Item::Tool {
                name,
                args_preview,
                status,
                result_preview,
                error,
                ..
            } => {
                // Errored tools keep their full card in BOTH modes (the
                // view's own rule: errors never hide behind a toggle) —
                // and "errored" is STATUS-based, matching the view
                // exactly (round-2 P1-1): `Failed` is minted from
                // `success: false` even with an EMPTY error string, the
                // failure text riding `result_preview` (the standard
                // shape for e.g. execute_command with a non-zero exit).
                // The error-string test alone dropped that evidence from
                // default archival exports. Clean tools stay one-line
                // summaries by default.
                let errored =
                    !error.is_empty() || matches!(status, ToolStatus::Failed | ToolStatus::Denied);
                if details || errored {
                    let mut card = format!(
                        "### {} {name} — {}",
                        status_glyph(*status),
                        status_word(*status)
                    );
                    if !args_preview.is_empty() {
                        let fence = fence_for(args_preview);
                        card.push_str(&format!("\n\n{fence}args\n{args_preview}\n{fence}"));
                    }
                    if !error.is_empty() {
                        card.push_str(&format!("\n\n**error:** {error}"));
                    } else if !result_preview.is_empty() {
                        let fence = fence_for(result_preview);
                        card.push_str(&format!("\n\n{fence}result\n{result_preview}\n{fence}"));
                    }
                    sections.push(card);
                } else {
                    let mut line = format!("- {} **{name}**", status_glyph(*status));
                    if *status != ToolStatus::Ok {
                        line.push_str(&format!(" · {}", status_word(*status)));
                    }
                    if !args_preview.is_empty() {
                        line.push_str(&format!(" — {}", inline_code(args_preview)));
                    }
                    sections.push(line);
                }
            }
            Item::Thinking {
                iteration,
                content,
                reasoning,
                ..
            } => {
                // Mirror the view: content when present, else reasoning.
                let body = if content.trim().is_empty() {
                    reasoning
                } else {
                    content
                };
                sections.push(format!("### ∴ Cycle {iteration}\n\n{}", quoted(body)));
            }
            Item::Probe { title, body } => {
                sections.push(format!("### ◈ {title}\n\n{}", quoted(body)));
            }
            Item::Image {
                artifact_id, label, ..
            } => {
                // Reference by id/label — never bytes.
                let label_part = if label.is_empty() {
                    String::new()
                } else {
                    format!(" **{label}**")
                };
                sections.push(format!("- image{label_part} (artifact `{artifact_id}`)"));
            }
            Item::Info { text } => {
                if is_wrapper_runtime_note(text) {
                    sections.push(format!("### ℹ Runtime wrapper note\n\n{}", quoted(text)));
                } else {
                    let mut lines = text.lines();
                    let first = lines.next().unwrap_or("");
                    let mut block = format!("> · {first}");
                    for l in lines {
                        block.push_str(&format!("\n> {l}"));
                    }
                    sections.push(block);
                }
            }
            Item::Error { text } => sections.push(format!("### ✗ Error\n\n{text}")),
        }
    }
    sections.join("\n\n") + "\n"
}

// ---------------------------------------------------------------------------
// JSONL (SFT)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct SftTurn {
    user: String,
    assistant: Option<String>,
    tools: Vec<Value>,
    cycles: Vec<Value>,
    steers: Vec<String>,
}

/// Segment items into turns with `Fold::chat_messages` pairing semantics
/// (parity-pinned by a test): a User opens a turn; the FIRST
/// `final_answer: true` assistant closes the NEWEST open turn; intra-turn
/// tools/cycles/steers attach to the open turn; everything with no open
/// turn is dropped.
fn segment_turns(items: &[Item]) -> Vec<SftTurn> {
    let mut turns: Vec<SftTurn> = Vec::new();
    for item in items {
        match item {
            Item::User { text } => turns.push(SftTurn {
                user: text.clone(),
                ..SftTurn::default()
            }),
            Item::Assistant {
                text,
                final_answer: true,
            } => {
                if let Some(last) = turns.last_mut() {
                    if last.assistant.is_none() {
                        last.assistant = Some(text.clone());
                    }
                }
            }
            Item::Tool {
                name,
                args_preview,
                status,
                result_preview,
                error,
                ..
            } => {
                if let Some(last) = turns.last_mut() {
                    if last.assistant.is_none() {
                        let mut t = Map::new();
                        t.insert("name".into(), json!(name));
                        t.insert("status".into(), json!(status_word(*status)));
                        if !args_preview.is_empty() {
                            t.insert("args_preview".into(), json!(args_preview));
                        }
                        if !result_preview.is_empty() {
                            t.insert("result_preview".into(), json!(result_preview));
                        }
                        if !error.is_empty() {
                            t.insert("error".into(), json!(error));
                        }
                        last.tools.push(Value::Object(t));
                    }
                }
            }
            Item::Thinking {
                iteration,
                content,
                reasoning,
                ..
            } => {
                if let Some(last) = turns.last_mut() {
                    if last.assistant.is_none() {
                        let mut c = Map::new();
                        c.insert("iteration".into(), json!(iteration));
                        if !content.is_empty() {
                            c.insert("content".into(), json!(content));
                        }
                        if !reasoning.is_empty() {
                            c.insert("reasoning".into(), json!(reasoning));
                        }
                        last.cycles.push(Value::Object(c));
                    }
                }
            }
            Item::Steer { text } => {
                if let Some(last) = turns.last_mut() {
                    if last.assistant.is_none() {
                        last.steers.push(text.clone());
                    }
                }
            }
            // Non-final assistant updates, images, info, errors: not part
            // of a trainable pair and not tool activity — markdown is the
            // archival surface for those.
            _ => {}
        }
    }
    turns
}

/// SFT lines + the count of SKIPPED (unanswered) turns. Each line is one
/// completed turn carrying the cumulative message prefix; with `details`,
/// a `details` side field describes THAT turn's intra-turn activity (the
/// prefix turns' details are on their own earlier lines). Default lines
/// carry ONLY `messages` — strictly drop-in trainable.
pub fn sft_lines(items: &[Item], details: bool) -> (Vec<String>, usize) {
    let turns = segment_turns(items);
    let mut lines = Vec::new();
    let mut prefix: Vec<Value> = Vec::new();
    let mut skipped = 0usize;
    for turn in &turns {
        let Some(answer) = &turn.assistant else {
            // A dangling user prompt is provider-hostile: skipped turns
            // never enter the file OR later lines' prefixes; the caller
            // reports the count to the user.
            skipped += 1;
            continue;
        };
        prefix.push(json!({"role": "user", "content": turn.user}));
        prefix.push(json!({"role": "assistant", "content": answer}));
        let mut line = Map::new();
        line.insert("messages".into(), Value::Array(prefix.clone()));
        if details {
            let mut d = Map::new();
            if !turn.tools.is_empty() {
                d.insert("tools".into(), Value::Array(turn.tools.clone()));
            }
            if !turn.cycles.is_empty() {
                d.insert("cycles".into(), Value::Array(turn.cycles.clone()));
            }
            if !turn.steers.is_empty() {
                d.insert("steers".into(), json!(turn.steers));
            }
            line.insert("details".into(), Value::Object(d));
        }
        lines.push(Value::Object(line).to_string());
    }
    (lines, skipped)
}

/// The required renderer shape: the joined JSONL document ("" when no
/// completed turns exist — callers refuse to write an empty file).
pub fn to_sft_jsonl(items: &[Item], details: bool) -> String {
    let (lines, _) = sft_lines(items, details);
    if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n") + "\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::Fold;

    fn user(text: &str) -> Item {
        Item::User { text: text.into() }
    }

    fn answer(text: &str) -> Item {
        Item::Assistant {
            text: text.into(),
            final_answer: true,
        }
    }

    fn tool(name: &str, status: ToolStatus, args: &str, result: &str, error: &str) -> Item {
        Item::Tool {
            key: format!("k-{name}"),
            name: name.into(),
            args_preview: args.into(),
            status,
            result_preview: result.into(),
            error: error.into(),
        }
    }

    fn thinking(iteration: u32, content: &str, reasoning: &str) -> Item {
        Item::Thinking {
            iteration,
            content: content.into(),
            reasoning: reasoning.into(),
            call: crate::transcript::CallCost::default(),
        }
    }

    fn sample_items() -> Vec<Item> {
        vec![
            Item::Info {
                text: "session acode-abc".into(),
            },
            user("write hello.txt"),
            thinking(1, "", "I should write the file"),
            tool(
                "write_file",
                ToolStatus::Ok,
                r#"{"path":"hello.txt"}"#,
                "ok",
                "",
            ),
            Item::Steer {
                text: "make it terse".into(),
            },
            answer("done — hello.txt written"),
            user("now delete it"),
            tool(
                "delete_file",
                ToolStatus::Failed,
                "",
                "",
                "denied by policy",
            ),
            Item::Error {
                text: "run failed".into(),
            },
        ]
    }

    // -- parse_args ---------------------------------------------------------

    #[test]
    fn parse_defaults_format_words_and_details() {
        assert_eq!(
            parse_args("").unwrap(),
            ExportArgs {
                format: ExportFormat::Markdown,
                details: false,
                path: None
            }
        );
        assert_eq!(parse_args("jsonl").unwrap().format, ExportFormat::Jsonl);
        assert_eq!(
            parse_args("MARKDOWN").unwrap().format,
            ExportFormat::Markdown
        );
        // --details anywhere among the tokens.
        let a = parse_args("--details jsonl /tmp/x.jsonl").unwrap();
        assert!(a.details);
        assert_eq!(a.format, ExportFormat::Jsonl);
        assert_eq!(a.path.as_deref(), Some("/tmp/x.jsonl"));
        let b = parse_args("md /tmp/x.md --details").unwrap();
        assert!(b.details);
        assert_eq!(b.format, ExportFormat::Markdown);
    }

    #[test]
    fn parse_refuses_unknown_flags_and_extra_paths() {
        // A typo'd flag must never become a filename.
        assert!(parse_args("--detials")
            .unwrap_err()
            .contains("unknown flag"));
        assert!(parse_args("a.md b.md")
            .unwrap_err()
            .contains("one output path"));
        // A format word AFTER the path is a second path, not a format.
        assert!(parse_args("report.md md")
            .unwrap_err()
            .contains("one output path"));
    }

    #[test]
    fn parse_extension_infers_and_conflicts_refuse() {
        assert_eq!(parse_args("out.jsonl").unwrap().format, ExportFormat::Jsonl);
        assert_eq!(parse_args("out.md").unwrap().format, ExportFormat::Markdown);
        // Unknown extension = markdown default.
        assert_eq!(
            parse_args("out.txt").unwrap().format,
            ExportFormat::Markdown
        );
        // Explicit word vs known extension disagreeing refuses.
        assert!(parse_args("md out.jsonl")
            .unwrap_err()
            .contains("conflicts"));
        assert!(parse_args("jsonl out.md")
            .unwrap_err()
            .contains("conflicts"));
        // Agreeing is fine.
        assert_eq!(
            parse_args("jsonl out.jsonl").unwrap().format,
            ExportFormat::Jsonl
        );
        // A file literally named `md` is expressible as ./md.
        assert_eq!(parse_args("./md").unwrap().path.as_deref(), Some("./md"));
    }

    // -- naming + paths -----------------------------------------------------

    #[test]
    fn default_filename_shape() {
        let name = default_filename(
            "acode-4c5ba1091cf3",
            "2026-07-24T15:59:03Z",
            ExportFormat::Markdown,
        );
        assert_eq!(name, "abstractcode-export-4c5ba109-20260724-155903.md");
        let name = default_filename("", "2026-07-24T15:59:03Z", ExportFormat::Jsonl);
        assert_eq!(name, "abstractcode-export-session-20260724-155903.jsonl");
    }

    #[test]
    fn resolve_output_path_refuses_tilde_and_keeps_explicit_verbatim() {
        assert!(
            resolve_output_path(Some("~/x.md"), "s", "t", ExportFormat::Markdown)
                .unwrap_err()
                .contains('~')
        );
        assert_eq!(
            resolve_output_path(Some("rel/x.md"), "s", "t", ExportFormat::Markdown).unwrap(),
            PathBuf::from("rel/x.md")
        );
        let auto = resolve_output_path(
            None,
            "acode-ff",
            "2026-01-02T03:04:05Z",
            ExportFormat::Jsonl,
        )
        .unwrap();
        assert_eq!(
            auto,
            PathBuf::from("abstractcode-export-ff-20260102-030405.jsonl")
        );
    }

    #[test]
    fn write_new_file_refuses_overwrite_and_missing_parent() {
        let dir = std::env::temp_dir().join(format!(
            "acode-export-unit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.md");
        write_new_file(&path, "hello").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
        // Second write to the same path refuses, content untouched.
        let err = write_new_file(&path, "clobber").unwrap_err();
        assert!(err.contains("never overwrites"), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
        // Missing parent refuses — never created silently.
        let missing = dir.join("no-such-dir").join("t.md");
        let err = write_new_file(&missing, "x").unwrap_err();
        assert!(err.contains("parent directory"), "{err}");
        assert!(!dir.join("no-such-dir").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // -- shared predicates ---------------------------------------------------

    #[test]
    fn has_conversation_is_false_for_info_only_folds() {
        assert!(!has_conversation(&[]));
        assert!(!has_conversation(&[Item::Info {
            text: "session x".into()
        }]));
        assert!(has_conversation(&[user("hi")]));
        assert!(has_conversation(&[Item::Error { text: "x".into() }]));
    }

    // -- markdown -------------------------------------------------------------

    #[test]
    fn markdown_header_roles_and_order() {
        let meta = ExportMeta {
            session_id: "acode-abc".into(),
            workflow: "basic-agent".into(),
            exported_at: "2026-07-24T16:00:00Z".into(),
            truncated: false,
        };
        let md = to_markdown(&sample_items(), &meta, false);
        assert!(md.starts_with("# AbstractCode transcript\n"));
        assert!(md.contains("- session: `acode-abc`"));
        assert!(md.contains("- workflow: basic-agent"));
        assert!(md.contains("- exported: 2026-07-24T16:00:00Z"));
        assert!(!md.contains("INCOMPLETE"), "no truncation note when whole");
        let user_pos = md.find("## User").unwrap();
        let asst_pos = md.find("## Assistant").unwrap();
        assert!(user_pos < asst_pos, "user before assistant");
        assert!(md.contains("write hello.txt"));
        assert!(md.contains("done — hello.txt written"));
        // Steers, errors, info visible in the default view.
        assert!(md.contains("### ↪ Steer"));
        assert!(md.contains("### ✗ Error"));
        assert!(md.contains("> · session acode-abc"));
        assert!(md.ends_with('\n'));
    }

    #[test]
    fn markdown_truncation_note_when_truncated() {
        let meta = ExportMeta {
            truncated: true,
            ..ExportMeta::default()
        };
        let md = to_markdown(&sample_items(), &meta, false);
        assert!(md.contains("INCOMPLETE: older items were dropped"));
    }

    #[test]
    fn markdown_details_gate() {
        let meta = ExportMeta::default();
        let clean = to_markdown(&sample_items(), &meta, false);
        let full = to_markdown(&sample_items(), &meta, true);
        // Thinking only in details; the view's content-else-reasoning rule.
        assert!(!clean.contains("Cycle 1"));
        assert!(full.contains("### ∴ Cycle 1"));
        assert!(full.contains("> I should write the file"));
        // Clean OK tool = one-line summary (no result); details = full card.
        assert!(clean.contains("- ✓ **write_file**"));
        assert!(!clean.contains("```result"));
        assert!(full.contains("### ✓ write_file — ok"));
        assert!(full.contains("```args"));
        assert!(full.contains("```result"));
        // Errored tool keeps its full card in BOTH modes.
        assert!(clean.contains("### ✗ delete_file — failed"));
        assert!(clean.contains("**error:** denied by policy"));
        // Item counts name the mode's own visibility.
        assert!(clean.contains("- items: 8 of 9 held by the client"));
        assert!(full.contains("- items: 9 of 9 held by the client"));
    }

    #[test]
    fn markdown_failed_tool_with_empty_error_keeps_its_result_in_clean_mode() {
        // Round-2 P1-1: `Failed` is minted from `success: false` even
        // with an EMPTY error string — the failure text rides
        // result_preview (execute_command with a non-zero exit is the
        // standard shape). The full card must survive a DEFAULT export;
        // the error-string test alone dropped exactly the evidence an
        // archival export exists to keep.
        let items = vec![
            user("run the tests"),
            tool(
                "execute_command",
                ToolStatus::Failed,
                r#"{"command":"cargo test"}"#,
                "assertion failed: left == right (exit code 101)",
                "", // empty error — the divergence-prone shape
            ),
            answer("the suite failed"),
        ];
        let clean = to_markdown(&items, &ExportMeta::default(), false);
        assert!(
            clean.contains("### ✗ execute_command — failed"),
            "status-based full card in CLEAN mode:\n{clean}"
        );
        assert!(
            clean.contains("assertion failed: left == right"),
            "the failure evidence survives the default export:\n{clean}"
        );
        assert!(clean.contains("```result"), "result fence used:\n{clean}");
    }

    #[test]
    fn markdown_fences_escalate_past_backticks_in_bodies() {
        let items = vec![
            user("run it"),
            tool(
                "execute_command",
                ToolStatus::Ok,
                "{\"command\":\"echo `date`\"}",
                "a ``` fence inside",
                "",
            ),
            answer("ran"),
        ];
        let md = to_markdown(&items, &ExportMeta::default(), true);
        // The result block's fence must be LONGER than the ``` inside it.
        assert!(md.contains("````result\na ``` fence inside\n````"), "{md}");
        // Inline code in the clean summary survives the backticked args.
        let clean = to_markdown(&items, &ExportMeta::default(), false);
        assert!(
            clean.contains("`` {\"command\":\"echo `date`\"} ``"),
            "{clean}"
        );
    }

    #[test]
    fn markdown_references_images_by_artifact_never_bytes() {
        let items = vec![
            user("draw"),
            Item::Image {
                run_id: "r1".into(),
                artifact_id: "art-9".into(),
                label: "diagram.png".into(),
            },
            answer("drawn"),
        ];
        let md = to_markdown(&items, &ExportMeta::default(), false);
        assert!(md.contains("- image **diagram.png** (artifact `art-9`)"));
    }

    #[test]
    fn markdown_wrapper_runtime_note_stays_separate_from_normal_info_lines() {
        let items = vec![
            user("do the work"),
            answer("done"),
            Item::Info {
                text: crate::runner::SUBRUN_CONCLUSION_NOTE.into(),
            },
        ];
        let md = to_markdown(&items, &ExportMeta::default(), false);
        assert!(md.contains("### ℹ Runtime wrapper note"), "{md}");
        assert!(md.contains(crate::runner::SUBRUN_CONCLUSION_NOTE), "{md}");
        assert!(
            !md.contains(&format!("> · {}", crate::runner::SUBRUN_CONCLUSION_NOTE)),
            "the wrapper note must not blend into the generic inline info stream:\n{md}"
        );
    }

    // -- jsonl ----------------------------------------------------------------

    #[test]
    fn jsonl_every_line_parses_with_cumulative_chat_schema() {
        let items = vec![user("q1"), answer("a1"), user("q2"), answer("a2")];
        let (lines, skipped) = sft_lines(&items, false);
        assert_eq!(lines.len(), 2);
        assert_eq!(skipped, 0);
        for (i, line) in lines.iter().enumerate() {
            let v: Value = serde_json::from_str(line).expect("valid JSON line");
            let obj = v.as_object().unwrap();
            // Default lines are strictly trainable: ONLY `messages`.
            assert_eq!(obj.keys().collect::<Vec<_>>(), vec!["messages"]);
            let msgs = obj["messages"].as_array().unwrap();
            // Cumulative prefix: line k carries 2*(k+1) messages.
            assert_eq!(msgs.len(), 2 * (i + 1));
            for (j, m) in msgs.iter().enumerate() {
                let want = if j % 2 == 0 { "user" } else { "assistant" };
                assert_eq!(m["role"], want, "alternating roles");
            }
        }
        // Last line ends with the newest answer.
        let last: Value = serde_json::from_str(&lines[1]).unwrap();
        let msgs = last["messages"].as_array().unwrap();
        assert_eq!(msgs.last().unwrap()["content"], "a2");
        // The wrapper joins with trailing newline.
        let doc = to_sft_jsonl(&items, false);
        assert_eq!(doc.lines().count(), 2);
        assert!(doc.ends_with('\n'));
    }

    #[test]
    fn jsonl_skips_incomplete_turns_and_matches_chat_messages_pairing() {
        // Turn 1 has no final answer (failed run) — excluded from the file
        // AND from later prefixes; turn 2 completes.
        let items = vec![
            user("q1-failed"),
            Item::Error {
                text: "run failed".into(),
            },
            user("q2"),
            answer("a2"),
            // A trailing final answer with NO open turn is ignored (parity).
            answer("stray"),
        ];
        let (lines, skipped) = sft_lines(&items, false);
        assert_eq!(lines.len(), 1);
        assert_eq!(skipped, 1);
        let v: Value = serde_json::from_str(&lines[0]).unwrap();
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["content"], "q2");
        assert_eq!(msgs[1]["content"], "a2");
        // Parity with Fold::chat_messages on the same items (the reuse
        // contract this module documents).
        let mut fold = Fold::new();
        for item in items.clone() {
            fold.push_item(item);
        }
        let pairs = fold.chat_messages(usize::MAX, usize::MAX);
        let flat: Vec<(String, String)> = msgs
            .iter()
            .map(|m| {
                (
                    m["role"].as_str().unwrap().to_string(),
                    m["content"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert_eq!(pairs, flat, "same pairing semantics as chat_messages");
    }

    #[test]
    fn jsonl_parity_holds_on_stacked_users_and_double_finals() {
        // The divergence-prone shapes (round-2 P2-3): (a) STACKED open
        // users — the answer closes the NEWEST turn (`turns.last_mut()`),
        // the older stays dangling; (b) a SECOND final answer in the same
        // turn — first wins (the `is_none()` guard). If a future edit
        // flips chat_messages to close the OLDEST open turn, this parity
        // pin fails instead of exports silently diverging from the
        // run-context seeding semantics.
        let items = vec![
            user("q-old"),
            user("q-new"),
            answer("a"),
            answer("dup-final"),
        ];
        let (lines, skipped) = sft_lines(&items, false);
        assert_eq!(lines.len(), 1);
        assert_eq!(skipped, 1, "the older stacked user stays dangling");
        let v: Value = serde_json::from_str(&lines[0]).unwrap();
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["content"], "q-new", "answer closes the NEWEST turn");
        assert_eq!(msgs[1]["content"], "a", "first final wins; dup ignored");
        let mut fold = Fold::new();
        for item in items {
            fold.push_item(item);
        }
        let pairs = fold.chat_messages(usize::MAX, usize::MAX);
        let flat: Vec<(String, String)> = msgs
            .iter()
            .map(|m| {
                (
                    m["role"].as_str().unwrap().to_string(),
                    m["content"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert_eq!(pairs, flat, "parity on the divergence-prone shapes");
    }

    #[test]
    fn jsonl_details_side_field_carries_the_closing_turns_activity() {
        let (lines, _) = sft_lines(&sample_items(), true);
        assert_eq!(lines.len(), 1, "one completed turn in the sample");
        let v: Value = serde_json::from_str(&lines[0]).unwrap();
        let obj = v.as_object().unwrap();
        assert!(obj.contains_key("details"));
        let d = &obj["details"];
        assert_eq!(d["tools"][0]["name"], "write_file");
        assert_eq!(d["tools"][0]["status"], "ok");
        assert_eq!(d["tools"][0]["args_preview"], r#"{"path":"hello.txt"}"#);
        assert_eq!(d["cycles"][0]["iteration"], 1);
        assert_eq!(d["cycles"][0]["reasoning"], "I should write the file");
        assert_eq!(d["steers"][0], "make it terse");
        // Default mode: no details key at all.
        let (plain, _) = sft_lines(&sample_items(), false);
        let v: Value = serde_json::from_str(&plain[0]).unwrap();
        assert!(!v.as_object().unwrap().contains_key("details"));
    }

    #[test]
    fn jsonl_empty_when_no_complete_turns() {
        let items = vec![
            user("q"),
            Item::Error {
                text: "boom".into(),
            },
        ];
        let (lines, skipped) = sft_lines(&items, false);
        assert!(lines.is_empty());
        assert_eq!(skipped, 1);
        assert_eq!(to_sft_jsonl(&items, false), "");
        // Errors-only transcript: nothing trainable either.
        let only_errors = vec![Item::Error { text: "x".into() }];
        assert_eq!(sft_lines(&only_errors, true).0.len(), 0);
    }
}
