//! Slash-command parsing for the composer.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    NewSession,
    Theme(Option<String>),
    Workflow,
    Model,
    Tools,
    /// `/permissions [read|write|all]` — THE tool-permission surface (the
    /// converged consolidation, c5028): one persisted per-session level.
    /// `None` reports the current level; `Some(raw)` sets it (refusing
    /// unknown spellings loudly). Replaces both `/tools tier` and the
    /// deleted `/auto` blanket — the blanket's three latent holes
    /// (ask-pin bypass, served-disabled-clamp bypass, empty-batch
    /// auto-approve) die with it.
    Permissions(Option<String>),
    /// `/workspace` — root / access mode / allowed paths modal.
    Workspace,
    Skills,
    Mcp,
    Cache,
    /// `None` opens the picker; `Some(id)` switches directly (the old
    /// separate `/session` command folded in — one surface, 2026-07-22).
    Sessions(Option<String>),
    /// `/details [full|fold]` — bare toggles the clean view; `full`
    /// expands thinking cards (content + labeled reasoning channel),
    /// `fold` returns to one-line gists (the default).
    Details(Option<String>),
    /// `/reasoning [level]` — the effort dial for the current route
    /// (stage 3 of the model picker, opened directly); with an argument,
    /// applies the level without the modal.
    Reasoning(Option<String>),
    /// `/gating [auto|wait]` — gating mode for gating-capable workflows
    /// (the multi-agent coder): auto runs unattended (skips human
    /// approval pauses), wait restores gated. Bare shows/toggles.
    Gating(Option<String>),
    /// `/status` — the run/session status card (client phase + server
    /// run status probe + connection + workspace facts).
    Status,
    /// `/history [n|all]` — stream the previous bloc of session turns
    /// from the gateway ledgers (boot loads only the last bloc; older
    /// history rapidly on request — the 2026-07-25 ruling).
    History(Option<String>),
    /// `/attach [path]` — stage a file for the NEXT plain-prompt send.
    /// `None` opens the pending manager (or prints usage when empty);
    /// `Some(rest)` is a path candidate (raw rest kept — paths contain
    /// spaces; `clear` resolved at dispatch, the /export precedent).
    Attach(Option<String>),
    /// `/export [md|markdown|jsonl] [--details] [path]` — transcript
    /// export (archival markdown / SFT JSONL). Parse keeps the raw rest
    /// (the /queue//goal//context convention); token semantics (format
    /// word, --details flag, output path) live in `crate::export` beside
    /// the renderers, where they are unit-tested without the UI.
    Export(Option<String>),
    Pause,
    Resume,
    Cancel,
    Steer(String),
    /// `/entities [name]` — roster modal; a name deep-links to its card.
    Entities(Option<String>),
    /// `/brain <name>` — open (or focus) a FLOW-BRAIN conversation with
    /// the entity: each message is one door summon of the entity-chat
    /// VisualFlow (continuity in the entity's graph; the view is
    /// session-local). `None` reports the focused conversation's brain.
    Brain(Option<String>),
    /// `/task <name> <title>` — durable task-inbox delegation (no visit).
    Task {
        name: String,
        title: String,
    },
    /// `/end [name] [reason]` — close a visit with closed_by=operator.
    End {
        name: Option<String>,
        reason: String,
    },
    /// `/focus <name|agent>` — explicit conversation focus switch.
    FocusSwitch(String),
    /// `/queue [text]` — queued prompts (worker-1 lane; variant added
    /// here because the parse below already referenced it mid-edit).
    Queue(Option<String>),
    /// `/goal [text]` — goal-agent runs (plan item 3). `None` = status;
    /// the exact word `stop` = cancel (dispatch decides — parse keeps the
    /// raw rest so "stop the noisy warnings" stays a goal text).
    Goal(Option<String>),
    /// `/gpu` — toggle the gateway-host GPU meter (OBS-6).
    Gpu,
    /// `/context [tokens|off]` — the operator-declared context window
    /// (CTX-0). `None` reports the current declaration + usage; a token
    /// count declares; `off`/`0` clears. Persisted.
    Context(Option<String>),
    /// `/redraw` — force a full-screen repaint (HDR-2; Ctrl+L twin).
    /// Recovers from external screen clears the damage tracker cannot see.
    Redraw,
    Quit,
    Unknown(String),
}

/// Parse composer text. `None` means "not a command — treat as a prompt".
pub fn parse(text: &str) -> Option<Command> {
    let t = text.trim();
    if !t.starts_with('/') {
        return None;
    }
    let mut parts = t.splitn(2, char::is_whitespace);
    let head = parts.next().unwrap_or("").to_lowercase();
    let rest = parts.next().unwrap_or("").trim().to_string();
    let cmd = match head.as_str() {
        "/help" | "/?" => Command::Help,
        "/new" | "/clear" => Command::NewSession,
        "/theme" | "/themes" => Command::Theme(if rest.is_empty() { None } else { Some(rest) }),
        "/workflow" | "/agent" | "/workflows" => Command::Workflow,
        "/model" | "/models" | "/provider" | "/providers" => Command::Model,
        // `/tools` opens the grants modal; the permission LEVEL moved to
        // its own `/permissions` surface (the c5028 consolidation).
        // `/tools tier` teaches the move instead of silently opening the
        // modal (muscle memory from the pre-consolidation spelling).
        "/tools" => {
            let mut words = rest.splitn(2, char::is_whitespace);
            match words.next() {
                Some("tier") => {
                    let arg = words.next().unwrap_or("").trim().to_string();
                    Command::Permissions(if arg.is_empty() { None } else { Some(arg) })
                }
                _ => Command::Tools,
            }
        }
        "/permissions" | "/permission" | "/perms" => {
            Command::Permissions(if rest.is_empty() { None } else { Some(rest) })
        }
        "/workspace" | "/ws" => Command::Workspace,
        "/skills" | "/skill" => Command::Skills,
        "/mcp" => Command::Mcp,
        "/cache" | "/caching" => Command::Cache,
        // `/session` stays as a spelling alias of `/sessions` (muscle
        // memory); one command, one behavior.
        "/sessions" | "/session" => {
            Command::Sessions(if rest.is_empty() { None } else { Some(rest) })
        }
        "/details" | "/detail" | "/verbose" => {
            Command::Details(if rest.is_empty() { None } else { Some(rest) })
        }
        "/reasoning" | "/thinking" => {
            Command::Reasoning(if rest.is_empty() { None } else { Some(rest) })
        }
        "/gating" | "/gate" => Command::Gating(if rest.is_empty() { None } else { Some(rest) }),
        "/status" => Command::Status,
        "/history" => Command::History(if rest.is_empty() { None } else { Some(rest) }),
        "/attach" => Command::Attach(if rest.is_empty() { None } else { Some(rest) }),
        "/export" => Command::Export(if rest.is_empty() { None } else { Some(rest) }),
        // The /auto blanket is DELETED (c5028): its spellings open the
        // permissions REPORT (teaches where the knob went) instead of
        // silently setting a now-PERSISTENT level the old toggle never
        // persisted — or vanishing into "unknown command".
        "/auto" | "/autoapprove" | "/approveall" => Command::Permissions(None),
        "/pause" => Command::Pause,
        "/resume" | "/continue" => Command::Resume,
        "/cancel" | "/stop" => Command::Cancel,
        "/steer" => Command::Steer(rest),
        // The whole rest is the prompt TEXT (no subcommands on purpose: a
        // queued prompt legitimately starts with words like "clear").
        // NO `/q` alias: that spelling belongs to /quit below.
        "/queue" => Command::Queue(if rest.is_empty() { None } else { Some(rest) }),
        "/goal" => Command::Goal(if rest.is_empty() { None } else { Some(rest) }),
        "/gpu" => Command::Gpu,
        "/context" | "/ctx" => Command::Context(if rest.is_empty() { None } else { Some(rest) }),
        "/redraw" => Command::Redraw,
        "/entities" | "/entity" => {
            Command::Entities(if rest.is_empty() { None } else { Some(rest) })
        }
        "/brain" => Command::Brain(if rest.is_empty() { None } else { Some(rest) }),
        "/task" => {
            // `/task <name> <title>` — both required; a missing half is
            // reported at dispatch (Unknown would hide the usage hint).
            let mut halves = rest.splitn(2, char::is_whitespace);
            let name = halves.next().unwrap_or("").trim().to_string();
            let title = halves.next().unwrap_or("").trim().to_string();
            Command::Task { name, title }
        }
        "/end" => {
            let mut halves = rest.splitn(2, char::is_whitespace);
            let name = halves.next().unwrap_or("").trim().to_string();
            let reason = halves.next().unwrap_or("").trim().to_string();
            Command::End {
                name: if name.is_empty() { None } else { Some(name) },
                reason,
            }
        }
        "/focus" => Command::FocusSwitch(rest),
        "/quit" | "/exit" | "/q" => Command::Quit,
        other => Command::Unknown(other.to_string()),
    };
    Some(cmd)
}

/// Canonical spellings + hints for the composer's `/` completion
/// dropdown (one entry per command, no aliases — the dropdown teaches
/// the canonical name; `parse` still accepts the aliases).
pub const COMPLETIONS: &[(&str, &str)] = &[
    ("help", "commands + keys"),
    ("new", "fresh session"),
    ("theme", "pick a theme"),
    ("workflow", "pick the agent workflow"),
    ("model", "pick provider + model + reasoning"),
    ("reasoning", "reasoning effort for the current route"),
    (
        "gating",
        "coder approval gating: auto (unattended) | wait (gated)",
    ),
    ("tools", "enable/disable gateway tools"),
    (
        "permissions",
        "tool permissions: read|write|all (sticky per session)",
    ),
    ("workspace", "workspace root, access mode, allowed paths"),
    ("skills", "attach gateway skills"),
    ("mcp", "MCP server registry"),
    ("cache", "prompt-cache + context status"),
    (
        "status",
        "run + session status card (client phase vs gateway run status)",
    ),
    (
        "history",
        "stream earlier session turns (boot loads the last bloc only)",
    ),
    ("sessions", "pick or set a session"),
    ("details", "show/hide reasoning (Ctrl+D)"),
    (
        "attach",
        "attach a file to your next message · bare /attach manages",
    ),
    (
        "export",
        "export the transcript: markdown or SFT JSONL · --details adds reasoning + tools",
    ),
    ("pause", "pause the run durably"),
    ("resume", "resume a paused run"),
    ("cancel", "cancel the active run"),
    ("steer", "steer the active run"),
    (
        "queue",
        "queue a prompt for after this run · bare /queue manages",
    ),
    (
        "goal",
        "run a goal to completion (goal workflow) · /goal stop cancels",
    ),
    ("gpu", "toggle the gateway-host GPU meter"),
    (
        "context",
        "declare the model context window (drives ctx N/M %) · /context off clears",
    ),
    ("redraw", "repaint the whole screen (Ctrl+L)"),
    ("entities", "entity roster + identity cards"),
    (
        "brain",
        "flow-brain conversation with an entity (summon-per-prompt)",
    ),
    ("task", "leave a task on an entity's desk"),
    ("end", "close an entity visit (reflection runs)"),
    ("focus", "switch conversation focus (Ctrl+E cycles)"),
    ("quit", "leave"),
];

/// Fuzzy completion over [`COMPLETIONS`] (UX-14, POLISH-1): prefix
/// matches rank FIRST (in table order — the muscle-memory hit stays on
/// top), then case-insensitive subsequence matches (`/wf` finds
/// `/workflow`, `/tt` finds `/tools tier`). Pure — the composer's `/`
/// trigger provider calls this instead of a bare `starts_with` filter.
pub fn completion_matches(query: &str) -> Vec<&'static (&'static str, &'static str)> {
    let q = query.to_lowercase();
    let mut prefix = Vec::new();
    let mut fuzzy = Vec::new();
    for entry in COMPLETIONS {
        if entry.0.starts_with(q.as_str()) {
            prefix.push(entry);
        } else if is_subsequence(&q, entry.0) {
            fuzzy.push(entry);
        }
    }
    prefix.append(&mut fuzzy);
    prefix
}

/// True when every char of `needle` appears in `hay` in order.
fn is_subsequence(needle: &str, hay: &str) -> bool {
    let mut hay_chars = hay.chars();
    needle.chars().all(|n| hay_chars.by_ref().any(|h| h == n))
}

pub const HELP_LINES: &[(&str, &str)] = &[
    (
        "<text> + Enter",
        "send a task to the agent (steers when a run is active)",
    ),
    ("/help", "this help"),
    (
        "/new",
        "fresh session (cancels an active run, new durable id)",
    ),
    (
        "/theme [id]",
        "pick a theme (26 built-in) or set one directly",
    ),
    (
        "/workflow",
        "pick the agent workflow from the gateway catalog",
    ),
    (
        "/model",
        "pick provider + model (default: the gateway routes)",
    ),
    ("/tools", "enable/disable gateway tools for your runs"),
    (
        "/permissions [t]",
        "tool permissions read|write|all — at-or-below auto-approves; sticky per session; bare reports",
    ),
    (
        "/workspace",
        "workspace root, access mode, allowed paths (what tools may touch)",
    ),
    ("/skills", "attach gateway skills to your runs"),
    ("/mcp", "show the gateway MCP server registry"),
    ("/cache", "prompt-cache + context status for the route"),
    (
        "/sessions [id]",
        "pick a recent session, or switch straight to an id",
    ),
    (
        "/details [full|fold]",
        "show/hide work detail (Ctrl+D); `full` expands thinking cards (content + reasoning), `fold` returns to one-line gists",
    ),
    (
        "/reasoning [level]",
        "reasoning effort for the current route: none|minimal|low|medium|high|xhigh|auto — bare /reasoning opens the dial; the model picker's third stage sets it too",
    ),
    (
        "/history [n|all]",
        "stream the previous bloc of this session's turns from the gateway, prepended in full detail — boot replays only the LAST bloc (--replay-turns sizes it); the stub line above the transcript names how many earlier turns exist",
    ),
    (
        "/status",
        "the status card: workflow, route, session, connection, client phase, run id, and a LIVE gateway run-status probe (server truth vs client view)",
    ),
    (
        "/attach [path]",
        "attach a file to your NEXT message (uploads at send; session uploads are permanent) — accepts ~, quotes, file:// spellings; bare /attach browses or manages pending; /attach clear discards; dropping a file onto the terminal attaches it directly (Ctrl+O undoes)",
    ),
    (
        "/export [fmt] [--details] [path]",
        "export the agent transcript to a file — md (default) for archival, jsonl for SFT training (one line per completed turn); --details adds reasoning + full tool cards; auto-names in the cwd, never overwrites",
    ),
    (
        "/pause",
        "pause the run durably on the gateway (survives quit)",
    ),
    ("/resume", "resume a paused run"),
    ("/cancel", "cancel the active run (Esc Esc does the same)"),
    ("/steer <text>", "explicitly steer the active run"),
    (
        "/queue [text]",
        "queue a prompt (FIFO): auto-runs after the current run succeeds; halts on failure/cancel; held by THIS CLIENT per session (prefs.json — other apps see runs only once started) and restores PAUSED; bare /queue opens the manager",
    ),
    (
        "/goal [text|stop]",
        "start a goal run — the goal workflow loops SERVER-side until verified done or max_cycles (one durable gateway run); bare /goal shows status; /goal stop cancels",
    ),
    (
        "/gpu",
        "toggle the gateway-host GPU meter (polls ~3s active / ~30s idle)",
    ),
    (
        "/context [n|off]",
        "declare the model context window in tokens (262144 / 262k) — the footer shows ctx used/window (%); labeled \"declared\", warns ≥75%; off clears; persisted",
    ),
    (
        "/redraw",
        "force a full-screen repaint (Ctrl+L) — recovers from a terminal clear (Cmd+K)",
    ),
    (
        "@name [text]",
        "talk with a summoned entity (bare @name opens; text sends a turn)",
    ),
    (
        "/entities [name]",
        "entity roster + identity card (cached; refreshes async)",
    ),
    (
        "/brain <name>",
        "flow-brain conversation: each message is one door summon (entity-chat flow); memory persists in the entity's graph, the view is session-local; bare reports the focused brain",
    ),
    (
        "/task <name> <title>",
        "leave a task on the entity's desk (works while asleep)",
    ),
    (
        "/end [name] [reason]",
        "close the entity visit (reflection runs; close restores sleep)",
    ),
    (
        "/focus <name|agent>",
        "switch conversation focus (Ctrl+E cycles)",
    ),
    ("/quit", "leave (Ctrl+Q too; Ctrl+C clears the prompt — twice in a row quits)"),
];

/// Key bindings + recovery lines for the help modal (keyboard truths that
/// are otherwise only discoverable by reading docs).
pub const HELP_EXTRA: &[(&str, &str)] = &[
    (
        "Esc",
        "clear the draft / defer an open prompt (Enter reopens) / when scrolled up, jump back to the live tail (that press never arms cancel)",
    ),
    (
        "PgUp / PgDn",
        "scroll the transcript (PgDn to the tail re-sticks)",
    ),
    (
        "Ctrl+L",
        "force a full-screen repaint (/redraw) — recovers from a terminal clear (Cmd+K)",
    ),
    (
        "?",
        "on an empty composer: open this reference (the footer's `? keys + commands`)",
    ),
    ("Tab", "move focus (composer / transcript / modal fields)"),
    ("Ctrl+T", "cycle theme"),
    (
        "select text",
        "drag to select in-app; release copies (OSC 52 clipboard) — Shift/Option-drag still selects natively",
    ),
    (
        "Ctrl+J",
        "newline — works in EVERY terminal (it is the LF byte on the legacy wire); Shift+Enter works where the kitty keyboard protocol is live (kitty/Ghostty/foot from startup; iTerm2 ≥ 3.5, VS Code/Cursor, Warp via the mid-session probe); Alt+Enter = Option+Enter with \"Option as Meta/Esc+\" on macOS",
    ),
    (
        "Ctrl+E",
        "cycle conversation focus (agent ↔ open entity visits)",
    ),
    (
        "entity turns",
        "non-interruptible mid-turn; Enter during a turn HOLDS the draft and sends it when the turn parks; mid-prompt @name completion inserts the name — the NEXT Enter submits",
    ),
    (
        "connection",
        "CLI: abstractcode-tui doctor / abstractcode-tui login",
    ),
    (
        "workspace",
        "gateway-managed by default: server policy clamps client paths; /workspace shows and extends the scope (mode + allowed paths persist in prefs.json)",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_not_a_command() {
        assert_eq!(parse("write a snake game"), None);
        assert_eq!(parse("  spaced prompt"), None);
    }

    #[test]
    fn capability_commands_parse() {
        assert_eq!(parse("/skills"), Some(Command::Skills));
        assert_eq!(parse("/mcp"), Some(Command::Mcp));
        assert_eq!(parse("/cache"), Some(Command::Cache));
        assert_eq!(parse("/sessions"), Some(Command::Sessions(None)));
        assert_eq!(parse("/tools"), Some(Command::Tools));
        assert_eq!(parse("/workspace"), Some(Command::Workspace));
        assert_eq!(parse("/ws"), Some(Command::Workspace));
    }

    #[test]
    fn permissions_parses_and_legacy_spellings_teach() {
        // The consolidated surface (c5028): /permissions is THE level
        // command; bare reports, an argument sets.
        assert_eq!(parse("/permissions"), Some(Command::Permissions(None)));
        assert_eq!(
            parse("/permissions write"),
            Some(Command::Permissions(Some("write".into())))
        );
        assert_eq!(
            parse("/perms all"),
            Some(Command::Permissions(Some("all".into())))
        );
        assert_eq!(
            parse("/permission read"),
            Some(Command::Permissions(Some("read".into())))
        );
        // Legacy `/tools tier` keeps working (muscle memory) — same
        // consolidated dispatch, word-boundary split preserved.
        assert_eq!(parse("/tools tier"), Some(Command::Permissions(None)));
        assert_eq!(
            parse("/tools tier write"),
            Some(Command::Permissions(Some("write".into())))
        );
        // "tierx" is NOT the tier subcommand; junk after /tools stays the
        // plain modal (which the user can navigate out of), never a
        // silent level change.
        assert_eq!(parse("/tools tierx"), Some(Command::Tools));
        assert_eq!(parse("/tools something"), Some(Command::Tools));
        // The deleted /auto blanket's spellings open the REPORT (teach
        // where the knob went) — never a silent state change, never
        // "unknown command".
        assert_eq!(parse("/auto"), Some(Command::Permissions(None)));
        assert_eq!(parse("/approveall"), Some(Command::Permissions(None)));
        assert_eq!(parse("/autoapprove"), Some(Command::Permissions(None)));
    }

    #[test]
    fn queue_parses_text_and_bare_form_and_never_steals_slash_q() {
        assert_eq!(parse("/queue"), Some(Command::Queue(None)));
        assert_eq!(
            parse("/queue run the tests"),
            Some(Command::Queue(Some("run the tests".into())))
        );
        // A queued prompt may start with any word — no subcommands.
        assert_eq!(
            parse("/queue clear the cache dir"),
            Some(Command::Queue(Some("clear the cache dir".into())))
        );
        // `/q` stays the QUIT alias (pre-existing muscle memory).
        assert_eq!(parse("/q"), Some(Command::Quit));
    }

    #[test]
    fn goal_parses_status_text_and_stop_word() {
        assert_eq!(parse("/goal"), Some(Command::Goal(None)));
        assert_eq!(
            parse("/goal make the suite green"),
            Some(Command::Goal(Some("make the suite green".into())))
        );
        // The exact word "stop" is the cancel verb (dispatch decides);
        // a goal that STARTS with "stop" keeps its whole text.
        assert_eq!(
            parse("/goal stop"),
            Some(Command::Goal(Some("stop".into())))
        );
        assert_eq!(
            parse("/goal stop the flaky retries"),
            Some(Command::Goal(Some("stop the flaky retries".into())))
        );
    }

    #[test]
    fn commands_parse_with_args() {
        assert_eq!(parse("/help"), Some(Command::Help));
        assert_eq!(
            parse("/theme nord"),
            Some(Command::Theme(Some("nord".into())))
        );
        assert_eq!(parse("/theme"), Some(Command::Theme(None)));
        assert_eq!(
            parse("/steer focus on tests"),
            Some(Command::Steer("focus on tests".into()))
        );
        // `/session` is a spelling alias of `/sessions` (one command).
        assert_eq!(
            parse("/session abc-123"),
            Some(Command::Sessions(Some("abc-123".into())))
        );
        assert_eq!(
            parse("/sessions abc-123"),
            Some(Command::Sessions(Some("abc-123".into())))
        );
        assert_eq!(parse("/nope"), Some(Command::Unknown("/nope".into())));
        assert_eq!(parse("/CANCEL"), Some(Command::Cancel));
    }

    #[test]
    fn help_and_completions_cover_every_command_exactly_once() {
        // Cycle-3 audit (item 5): three lanes appended commands
        // concurrently — pin that every new command surface appears in
        // BOTH teaching surfaces exactly once, every completion entry
        // parses to a real command, and no duplicates crept in.
        let completion_heads: Vec<&str> = COMPLETIONS.iter().map(|(c, _)| *c).collect();
        let help_heads: Vec<&str> = HELP_LINES.iter().map(|(k, _)| *k).collect();
        // Every completion candidate must parse as a KNOWN command (the
        // dropdown teaching an Unknown spelling would be a lie).
        for head in &completion_heads {
            let cmd = parse(&format!("/{head}")).expect("completion is a slash command");
            assert!(
                !matches!(cmd, Command::Unknown(_)),
                "/{head} from COMPLETIONS parses as Unknown"
            );
        }
        // No duplicate completion entries.
        let mut seen = std::collections::HashSet::new();
        for head in &completion_heads {
            assert!(seen.insert(*head), "duplicate completion entry: {head}");
        }
        // Every lane's new command appears in BOTH surfaces exactly once.
        for needle in [
            "queue",
            "goal",
            "gpu",
            "context",
            "redraw",
            "workspace",
            "entities",
            "task",
            "end",
            "focus",
            "permissions",
            "export",
            "attach",
            "status",
            "history",
        ] {
            assert_eq!(
                completion_heads.iter().filter(|c| **c == needle).count(),
                1,
                "{needle} exactly once in COMPLETIONS"
            );
            assert_eq!(
                help_heads
                    .iter()
                    .filter(|k| {
                        // Help keys carry usage decoration ("/queue [text]").
                        k.trim_start_matches('/')
                            .split_whitespace()
                            .take(needle.split_whitespace().count())
                            .collect::<Vec<_>>()
                            .join(" ")
                            == needle
                    })
                    .count(),
                1,
                "{needle} exactly once in HELP_LINES"
            );
        }
        // And no duplicate help keys at all.
        let mut seen_help = std::collections::HashSet::new();
        for k in &help_heads {
            assert!(seen_help.insert(*k), "duplicate help line: {k}");
        }
    }

    #[test]
    fn export_parses_with_raw_rest() {
        // Token semantics (format word, --details, path) live in
        // `crate::export::parse_args` and are unit-tested there; parse
        // keeps the raw rest like /queue//goal//context.
        assert_eq!(parse("/export"), Some(Command::Export(None)));
        assert_eq!(
            parse("/export jsonl --details /tmp/t.jsonl"),
            Some(Command::Export(Some("jsonl --details /tmp/t.jsonl".into())))
        );
        assert_eq!(
            parse("/EXPORT md"),
            Some(Command::Export(Some("md".into()))),
            "head is case-insensitive, rest verbatim"
        );
    }

    #[test]
    fn attach_parses_with_raw_rest() {
        // Paths contain spaces — the rest stays verbatim; `clear` and
        // spelling expansion resolve at dispatch (the /export precedent).
        assert_eq!(parse("/attach"), Some(Command::Attach(None)));
        assert_eq!(
            parse("/attach ~/My Report.pdf"),
            Some(Command::Attach(Some("~/My Report.pdf".into())))
        );
        assert_eq!(
            parse("/attach clear"),
            Some(Command::Attach(Some("clear".into())))
        );
    }

    #[test]
    fn context_and_redraw_parse() {
        assert_eq!(parse("/context"), Some(Command::Context(None)));
        assert_eq!(
            parse("/context 262k"),
            Some(Command::Context(Some("262k".into())))
        );
        assert_eq!(
            parse("/ctx off"),
            Some(Command::Context(Some("off".into())))
        );
        assert_eq!(parse("/redraw"), Some(Command::Redraw));
    }

    #[test]
    fn gpu_toggle_parses() {
        assert_eq!(parse("/gpu"), Some(Command::Gpu));
        // Trailing junk still toggles (no argument surface to protect).
        assert_eq!(parse("/gpu on"), Some(Command::Gpu));
    }

    #[test]
    fn completion_matching_is_fuzzy_with_prefix_ranked_first() {
        // POLISH-1 / UX-14: `/wf` finds `/workflow` (subsequence).
        let hits: Vec<&str> = completion_matches("wf").iter().map(|(c, _)| *c).collect();
        assert!(hits.contains(&"workflow"), "wf → workflow: {hits:?}");
        // Prefix matches rank before fuzzy ones: "t" puts the t-prefixed
        // commands (theme/tools/task/…) ahead of mere-subsequence hits.
        let hits: Vec<&str> = completion_matches("t").iter().map(|(c, _)| *c).collect();
        let first_fuzzy = hits
            .iter()
            .position(|c| !c.starts_with('t'))
            .unwrap_or(hits.len());
        assert!(
            hits[..first_fuzzy].iter().all(|c| c.starts_with('t'))
                && hits[..first_fuzzy].len() >= 3,
            "prefix block first: {hits:?}"
        );
        // Subsequence matching still finds mid-word targets ("prm" →
        // permissions; the old multi-word "tt" example died with the
        // /tools tier completion row).
        let hits: Vec<&str> = completion_matches("prm").iter().map(|(c, _)| *c).collect();
        assert!(hits.contains(&"permissions"), "{hits:?}");
        // Empty query = the full table in table order.
        assert_eq!(completion_matches("").len(), COMPLETIONS.len());
        // No match = empty, never a panic.
        assert!(completion_matches("zzzz").is_empty());
        // Case-insensitive.
        assert!(completion_matches("WF")
            .iter()
            .any(|(c, _)| *c == "workflow"));
    }

    #[test]
    fn entity_commands_parse() {
        assert_eq!(parse("/entities"), Some(Command::Entities(None)));
        assert_eq!(
            parse("/entities castor"),
            Some(Command::Entities(Some("castor".into())))
        );
        assert_eq!(
            parse("/task castor look at the door logs"),
            Some(Command::Task {
                name: "castor".into(),
                title: "look at the door logs".into()
            })
        );
        assert_eq!(
            parse("/task castor"),
            Some(Command::Task {
                name: "castor".into(),
                title: String::new()
            }),
            "missing title parses; dispatch reports usage"
        );
        assert_eq!(
            parse("/end"),
            Some(Command::End {
                name: None,
                reason: String::new()
            })
        );
        assert_eq!(
            parse("/end castor thanks for the check"),
            Some(Command::End {
                name: Some("castor".into()),
                reason: "thanks for the check".into()
            })
        );
        assert_eq!(
            parse("/focus agent"),
            Some(Command::FocusSwitch("agent".into()))
        );
    }
}
