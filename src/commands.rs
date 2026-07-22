//! Slash-command parsing for the composer.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    NewSession,
    Theme(Option<String>),
    Workflow,
    Model,
    Tools,
    Skills,
    Mcp,
    Cache,
    Sessions,
    Session(Option<String>),
    Details,
    AutoApprove,
    Pause,
    Resume,
    Cancel,
    Steer(String),
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
        "/tools" => Command::Tools,
        "/skills" | "/skill" => Command::Skills,
        "/mcp" => Command::Mcp,
        "/cache" | "/caching" => Command::Cache,
        "/sessions" => Command::Sessions,
        "/session" => Command::Session(if rest.is_empty() { None } else { Some(rest) }),
        "/details" | "/detail" | "/verbose" => Command::Details,
        "/auto" | "/autoapprove" | "/approveall" => Command::AutoApprove,
        "/pause" => Command::Pause,
        "/resume" | "/continue" => Command::Resume,
        "/cancel" | "/stop" => Command::Cancel,
        "/steer" => Command::Steer(rest),
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
    ("model", "pick provider + model"),
    ("tools", "enable/disable gateway tools"),
    ("skills", "attach gateway skills"),
    ("mcp", "MCP server registry"),
    ("cache", "prompt-cache + context status"),
    ("sessions", "pick a recent session"),
    ("session", "show or set the session id"),
    ("details", "show/hide reasoning (Ctrl+D)"),
    ("auto", "toggle auto-approve (session)"),
    ("pause", "pause the run durably"),
    ("resume", "resume a paused run"),
    ("cancel", "cancel the active run"),
    ("steer", "steer the active run"),
    ("quit", "leave"),
];

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
    ("/skills", "attach gateway skills to your runs"),
    ("/mcp", "show the gateway MCP server registry"),
    ("/cache", "prompt-cache + context status for the route"),
    ("/sessions", "pick a recent session to continue"),
    ("/session [id]", "show or set the durable session id"),
    ("/details", "show/hide reasoning + tool results (Ctrl+D)"),
    (
        "/auto",
        "toggle auto-approve of tool batches (session-scoped)",
    ),
    (
        "/pause",
        "pause the run durably on the gateway (survives quit)",
    ),
    ("/resume", "resume a paused run"),
    ("/cancel", "cancel the active run (Esc Esc does the same)"),
    ("/steer <text>", "explicitly steer the active run"),
    ("/quit", "leave (Ctrl+C / Ctrl+Q work too)"),
];

/// Key bindings + recovery lines for the help modal (keyboard truths that
/// are otherwise only discoverable by reading docs).
pub const HELP_EXTRA: &[(&str, &str)] = &[
    (
        "Esc",
        "clear the draft / defer an open prompt (Enter reopens)",
    ),
    (
        "PgUp / PgDn",
        "scroll the transcript (PgDn to the tail re-sticks)",
    ),
    ("Tab", "move focus (composer / transcript / modal fields)"),
    ("Ctrl+T", "cycle theme"),
    (
        "select text",
        "drag to select in-app; release copies (OSC 52 clipboard) — Shift/Option-drag still selects natively",
    ),
    (
        "Alt+Enter",
        "newline in the composer (Shift+Enter on kitty terminals); ↑/↓ recall sent messages",
    ),
    (
        "connection",
        "CLI: abstractcode-tui doctor / abstractcode-tui login",
    ),
    (
        "workspace",
        "gateway-managed by default: server policy clamps client paths; files land in the gateway's workspace root or a managed per-session folder (--workspace asks; the gateway decides)",
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
        assert_eq!(parse("/sessions"), Some(Command::Sessions));
        assert_eq!(parse("/tools"), Some(Command::Tools));
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
        assert_eq!(
            parse("/session abc-123"),
            Some(Command::Session(Some("abc-123".into())))
        );
        assert_eq!(parse("/nope"), Some(Command::Unknown("/nope".into())));
        assert_eq!(parse("/CANCEL"), Some(Command::Cancel));
    }
}
