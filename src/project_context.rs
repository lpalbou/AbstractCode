//! Project instructions (`AGENTS.md`) for gateway agent runs — CLIENT-SIDE
//! discovery, server-side application.
//!
//! Parity with the Python `abstractcode` client, which composes
//! `_runtime.system_prompt_extra` from `[project context][skills block]`
//! (`abstractcode/react_shell.py:12972-13000`) so the agent reads the
//! project's own conventions before it writes a line. This client sent NO
//! system prompt and NO project context at all (`StartOpts.system` was
//! hardcoded empty at both call sites), so the server-side agent coded blind
//! about the repo it was pointed at — a first-order quality gap, not a
//! cosmetic one.
//!
//! Wire key: `_runtime.system_prompt_extra`. Deliberately the SAME key
//! abstractcode already uses end-to-end, so no gateway or bundle change is
//! needed for a NATIVE-LOOP bundle (react/codeact/memact), whose root run
//! vars are the loop's own vars.
//!
//! It does NOT reach flow-graph Agent children: the runtime compiler rebuilds
//! each child `_runtime` and inherits a fixed set carrying `thinking` but not
//! this key (`abstractruntime/.../compiler.py:1347-1396`). So for
//! `basic-agent` — the default workflow — the project context reaches the root
//! and stops there, pending the one-row compiler change requested in
//! `docs/reports/2026-07-30-abstractcode-parity.md`.
//!
//! Discovery mirrors the Python implementation: nearest `AGENTS.md` walking
//! the workspace root UP to the git root (a sub-project overrides the
//! monorepo file), plus a user-global `~/.abstract/AGENTS.md`, composed
//! global-first so the project can override.
//!
//! The rendered block is deterministic for fixed file contents: its bytes
//! change only when the files change, keeping the cached system-prompt
//! prefix stable across cycles (prompt-cache discipline).

use std::path::{Path, PathBuf};

pub const PROJECT_CONTEXT_FILENAME: &str = "AGENTS.md";

/// An `AGENTS.md` beyond this size almost certainly holds machine history,
/// not instructions; refuse honestly rather than silently ballooning every
/// prompt. Same cap as the Python client.
pub const MAX_PROJECT_CONTEXT_CHARS: usize = 200_000;

/// Discovered project instructions, ready to ride the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectContext {
    /// Composition order — the paths that contributed, for the UI notice.
    pub sources: Vec<String>,
    pub text: String,
}

impl ProjectContext {
    pub fn char_count(&self) -> usize {
        self.text.chars().count()
    }
}

/// Why no project context rode this run — each variant is REPORTABLE, never
/// swallowed. Silence about a dropped 200 KB instruction file is exactly how
/// an agent ends up ignoring conventions nobody noticed it never read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextSkip {
    /// No `AGENTS.md` anywhere on the search path — the ordinary case.
    NotFound,
    /// Found, but over the cap. Carries the message to show the operator.
    TooLarge { chars: usize, sources: Vec<String> },
}

fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    for _ in 0..50 {
        if cur.join(".git").exists() {
            return Some(cur);
        }
        match cur.parent() {
            Some(p) if p != cur => cur = p.to_path_buf(),
            _ => return None,
        }
    }
    None
}

/// Nearest `AGENTS.md` walking `workspace_root` → git root (or the workspace
/// alone when not in a repo — walking past a non-repo root would read
/// unrelated files off the operator's disk).
fn nearest_project_file(workspace_root: &Path) -> Option<PathBuf> {
    let start = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let stop = find_git_root(&start).unwrap_or_else(|| start.clone());
    let mut cur = start;
    for _ in 0..50 {
        let candidate = cur.join(PROJECT_CONTEXT_FILENAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        if cur == stop {
            return None;
        }
        match cur.parent() {
            Some(p) if p != cur => cur = p.to_path_buf(),
            _ => return None,
        }
    }
    None
}

/// Load and compose project instructions for `workspace_root`.
///
/// `user_global` defaults to `~/.abstract/AGENTS.md`; pass it explicitly in
/// tests so a developer's real home file can never change a test outcome.
pub fn load_project_context(
    workspace_root: &Path,
    user_global: Option<&Path>,
) -> Result<ProjectContext, ContextSkip> {
    let home_global = user_global.map(PathBuf::from).or_else(|| {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|h| h.join(".abstract").join(PROJECT_CONTEXT_FILENAME))
    });

    let mut parts: Vec<(String, String)> = Vec::new();
    if let Some(g) = home_global.as_deref() {
        if g.is_file() {
            if let Ok(body) = std::fs::read_to_string(g) {
                parts.push((g.display().to_string(), body));
            }
        }
    }
    if let Some(p) = nearest_project_file(workspace_root) {
        if let Ok(body) = std::fs::read_to_string(&p) {
            parts.push((p.display().to_string(), body));
        }
    }

    let parts: Vec<(String, String)> = parts
        .into_iter()
        .map(|(src, body)| (src, body.trim().to_string()))
        .filter(|(_, body)| !body.is_empty())
        .collect();
    if parts.is_empty() {
        return Err(ContextSkip::NotFound);
    }

    let total: usize = parts.iter().map(|(_, b)| b.chars().count()).sum();
    let sources: Vec<String> = parts.iter().map(|(s, _)| s.clone()).collect();
    if total > MAX_PROJECT_CONTEXT_CHARS {
        return Err(ContextSkip::TooLarge {
            chars: total,
            sources,
        });
    }

    let mut rendered = String::from("Project instructions (from AGENTS.md — follow them):");
    for (src, body) in &parts {
        rendered.push_str(&format!("\n--- {src} ---\n{body}"));
    }
    Ok(ProjectContext {
        sources,
        text: rendered,
    })
}

/// Resolve the `system_prompt_extra` payload for a run — the ONE decision
/// shared by headless `exec` and the interactive TUI, so the two surfaces can
/// never drift into injecting different context for the same workspace.
///
/// Reporting is injected: `warn` takes a skip that is worth saying out loud,
/// `report` takes `(sources, chars)` for a successful injection. Both are
/// call-site rendering (stderr for exec, a toast for the TUI) — this function
/// owns only the decision. Returns the payload, or an empty string when
/// nothing should ride.
pub fn resolve_project_context(
    workspace_root: Option<&str>,
    opted_out: bool,
    mut warn: impl FnMut(String),
    mut report: impl FnMut(String, usize),
) -> String {
    if opted_out {
        return String::new();
    }
    // No workspace = no project to read conventions from.
    let root = match workspace_root.map(str::trim).filter(|r| !r.is_empty()) {
        Some(r) => PathBuf::from(r),
        None => return String::new(),
    };
    // A workspace root that is not on THIS machine (a remote gateway's own
    // path) is not readable here; that is not an error worth a warning.
    if !root.is_dir() {
        return String::new();
    }
    match load_project_context(&root, None) {
        Ok(ctx) => {
            report(ctx.sources.join(", "), ctx.char_count());
            ctx.text
        }
        Err(skip) => {
            if let Some(line) = skip_notice(&skip) {
                warn(line);
            }
            String::new()
        }
    }
}

/// The operator-facing line for a skip that is worth saying out loud.
/// `None` = nothing to report (no file is the ordinary case).
pub fn skip_notice(skip: &ContextSkip) -> Option<String> {
    match skip {
        ContextSkip::NotFound => None,
        ContextSkip::TooLarge { chars, sources } => Some(format!(
            "AGENTS.md not injected — project context is {chars} chars (cap {MAX_PROJECT_CONTEXT_CHARS}); trim {} or split instructions",
            sources.join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmpdir(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "acode-tui-pctx-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&base).unwrap();
        base
    }

    /// A workspace with no AGENTS.md and no global reports NotFound — and
    /// NotFound is silent (`skip_notice` → None): the ordinary case must not
    /// produce a scary line on every run.
    #[test]
    fn absent_context_is_not_found_and_silent() {
        let ws = tmpdir("absent");
        let missing_global = ws.join("no-such-global.md");
        let err = load_project_context(&ws, Some(&missing_global)).unwrap_err();
        assert_eq!(err, ContextSkip::NotFound);
        assert!(skip_notice(&err).is_none());
    }

    /// The rendered block names its source and carries the file body under
    /// the same header the Python client uses (cross-client parity: an
    /// operator reading either transcript sees the same framing).
    #[test]
    fn project_file_renders_with_header_and_source() {
        let ws = tmpdir("project");
        fs::write(ws.join("AGENTS.md"), "Always run cargo fmt.\n").unwrap();
        let missing_global = ws.join("no-such-global.md");
        let ctx = load_project_context(&ws, Some(&missing_global)).unwrap();
        assert!(ctx
            .text
            .starts_with("Project instructions (from AGENTS.md — follow them):"));
        assert!(ctx.text.contains("Always run cargo fmt."));
        assert_eq!(ctx.sources.len(), 1);
        assert!(ctx.sources[0].ends_with("AGENTS.md"));
    }

    /// Global first, project second — so a project file can override the
    /// user's global conventions (the Python composition order).
    #[test]
    fn global_composes_before_project() {
        let ws = tmpdir("both");
        fs::write(ws.join("AGENTS.md"), "PROJECT-RULE").unwrap();
        let global = ws.join("global-AGENTS.md");
        fs::write(&global, "GLOBAL-RULE").unwrap();
        let ctx = load_project_context(&ws, Some(&global)).unwrap();
        let gi = ctx.text.find("GLOBAL-RULE").expect("global present");
        let pi = ctx.text.find("PROJECT-RULE").expect("project present");
        assert!(gi < pi, "global composes before project");
        assert_eq!(ctx.sources.len(), 2);
    }

    /// Nearest file wins: a sub-project's AGENTS.md overrides the repo root's
    /// so a monorepo package can state its own rules.
    #[test]
    fn nearest_file_wins_walking_up_to_the_git_root() {
        let repo = tmpdir("nearest");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(repo.join("AGENTS.md"), "ROOT-RULE").unwrap();
        let sub = repo.join("packages").join("inner");
        fs::create_dir_all(&sub).unwrap();
        let missing_global = repo.join("no-such-global.md");

        // No file in the sub-project: the walk reaches the repo root.
        let from_sub = load_project_context(&sub, Some(&missing_global)).unwrap();
        assert!(from_sub.text.contains("ROOT-RULE"));

        // A nearer file shadows it entirely (ONE project file, not both).
        fs::write(sub.join("AGENTS.md"), "SUB-RULE").unwrap();
        let shadowed = load_project_context(&sub, Some(&missing_global)).unwrap();
        assert!(shadowed.text.contains("SUB-RULE"));
        assert!(
            !shadowed.text.contains("ROOT-RULE"),
            "nearest wins outright — the walk stops at the first hit"
        );
    }

    /// The walk must not escape a non-repo workspace: a file one level above
    /// an un-versioned directory is NOT this project's instructions.
    #[test]
    fn walk_stops_at_the_workspace_when_not_a_repo() {
        let outer = tmpdir("norepo");
        fs::write(outer.join("AGENTS.md"), "OUTSIDE-RULE").unwrap();
        let ws = outer.join("workspace");
        fs::create_dir_all(&ws).unwrap();
        let missing_global = outer.join("no-such-global.md");
        assert_eq!(
            load_project_context(&ws, Some(&missing_global)).unwrap_err(),
            ContextSkip::NotFound
        );
    }

    /// Oversized context is REFUSED and REPORTED, never silently truncated
    /// or silently dropped.
    #[test]
    fn oversized_context_refuses_loudly() {
        let ws = tmpdir("huge");
        fs::write(
            ws.join("AGENTS.md"),
            "x".repeat(MAX_PROJECT_CONTEXT_CHARS + 1),
        )
        .unwrap();
        let missing_global = ws.join("no-such-global.md");
        let err = load_project_context(&ws, Some(&missing_global)).unwrap_err();
        match &err {
            ContextSkip::TooLarge { chars, sources } => {
                assert!(*chars > MAX_PROJECT_CONTEXT_CHARS);
                assert_eq!(sources.len(), 1);
            }
            other => panic!("expected TooLarge, got {other:?}"),
        }
        let notice = skip_notice(&err).expect("oversized context is reported");
        assert!(notice.contains("not injected"));
    }

    /// Byte-stability for fixed contents — the prompt-cache contract.
    #[test]
    fn rendering_is_deterministic() {
        let ws = tmpdir("stable");
        fs::write(ws.join("AGENTS.md"), "rule one\nrule two").unwrap();
        let missing_global = ws.join("no-such-global.md");
        let a = load_project_context(&ws, Some(&missing_global)).unwrap();
        let b = load_project_context(&ws, Some(&missing_global)).unwrap();
        assert_eq!(a.text, b.text);
    }
}
