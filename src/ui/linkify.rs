//! OSC-8 hyperlinks for run-derived text: file paths and URLs in tool
//! cards become clickable (operator ask, 2026-08-28 — "file paths and
//! url i would say").
//!
//! Design rules:
//!
//! - **Zero visual change.** A linked token keeps its ink — no
//!   underline, no color shift. The affordance is the terminal's own
//!   (iTerm2/kitty/WezTer/Ghostty underline OSC-8 links on hover);
//!   terminals without OSC 8 ignore the bytes. The transcript never
//!   gets busier for this.
//! - **Never a fabricated target.** A URL links as itself. An absolute
//!   path links as `file://` verbatim. A RELATIVE path links only when
//!   it resolves to a file that EXISTS under the workspace root — tool
//!   output from a gateway-managed remote workspace must not become a
//!   local link to nothing. The existence check runs in the draw path
//!   deliberately: it is one `stat` per path-shaped token per damage
//!   repaint (not per tick — an idle transcript repaints nothing), and
//!   it is what keeps the link honest.
//! - **Token-bounded.** Classification is per whitespace token, so a
//!   wrapped line can at worst split a token and lose its link — a
//!   degraded affordance, never a wrong screen.

use std::rc::Rc;

use abstracttui::prelude::*;
use abstracttui::render::Style;
use abstracttui::ui::StyledCanvas;

/// One run of a line: plain text, or a linkable core with its URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seg {
    Plain(String),
    Link { text: String, uri: String },
}

/// Punctuation a sentence hangs on a token's tail — never part of the
/// path/URL itself. Trimmed iteratively (`(see http://x));`).
const TRAIL: &[char] = &[',', ')', ';', ':', '.', '\'', '"', ']', '}', '>', '!', '?'];
/// Wrappers a token arrives inside.
const LEAD: &[char] = &['(', '\'', '"', '[', '{', '<'];

/// Split `line` into printable segments, linking URL and path tokens.
/// `root` resolves relative paths (`None` = relative paths never link).
pub fn segments(line: &str, root: Option<&str>) -> Vec<Seg> {
    let mut out: Vec<Seg> = Vec::new();
    let mut plain = String::new();
    let mut rest = line;
    while !rest.is_empty() {
        // Take one whitespace run, then one token.
        let tok_start = rest
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(rest.len());
        plain.push_str(&rest[..tok_start]);
        rest = &rest[tok_start..];
        if rest.is_empty() {
            break;
        }
        let tok_end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
        let tok = &rest[..tok_end];
        rest = &rest[tok_end..];
        match classify(tok, root) {
            Some((lead, core_len, uri)) => {
                plain.push_str(&tok[..lead]);
                if !plain.is_empty() {
                    out.push(Seg::Plain(std::mem::take(&mut plain)));
                }
                out.push(Seg::Link {
                    text: tok[lead..lead + core_len].to_string(),
                    uri,
                });
                plain.push_str(&tok[lead + core_len..]);
            }
            None => plain.push_str(tok),
        }
    }
    if !plain.is_empty() {
        out.push(Seg::Plain(plain));
    }
    out
}

/// Classify one token: `Some((leading_junk_len, core_len, uri))`.
fn classify(tok: &str, root: Option<&str>) -> Option<(usize, usize, String)> {
    // Strip wrappers so `(src/ui/mod.rs)` and `"http://x"` both link.
    let lead = tok.len() - tok.trim_start_matches(LEAD).len();
    let mut core = &tok[lead..];
    loop {
        let trimmed = core.trim_end_matches(TRAIL);
        if trimmed.len() == core.len() {
            break;
        }
        core = trimmed;
    }
    if core.is_empty() {
        return None;
    }
    if let Some(uri) = url_uri(core) {
        return Some((lead, core.len(), uri));
    }
    file_uri(core, root).map(|uri| (lead, core.len(), uri))
}

/// An http(s) URL links as itself. The trailing-punctuation trim above
/// already happened, so `https://x.dev/a).` linked its real extent.
fn url_uri(core: &str) -> Option<String> {
    let after = core
        .strip_prefix("https://")
        .or_else(|| core.strip_prefix("http://"))?;
    (!after.is_empty()).then(|| core.to_string())
}

/// A path-shaped token becomes a `file://` URI, or nothing.
///
/// Path-shaped = carries a `/`, is not a URL, and holds no `=` (a
/// `--flag=a/b` token is an argument, not a path the user reaches
/// for). A `:12` / `:12:5` line-anchor suffix stays in the TEXT but
/// leaves the URI — `file://` has no line grammar that terminals
/// agree on. Absolute paths link verbatim; relative ones only when
/// they exist under `root` (the honesty rule in the module doc).
fn file_uri(core: &str, root: Option<&str>) -> Option<String> {
    if !core.contains('/') || core.contains("://") || core.contains('=') || core.starts_with('-') {
        return None;
    }
    let path = strip_line_anchor(core);
    if path.is_empty() || path == "/" {
        return None;
    }
    if path.starts_with('/') {
        return Some(format!("file://{path}"));
    }
    if let Some(tail) = path.strip_prefix("~/") {
        let home = std::env::var("HOME").ok()?;
        let abs = format!("{}/{tail}", home.trim_end_matches('/'));
        return Some(format!("file://{abs}"));
    }
    let root = root?;
    let abs = format!("{}/{path}", root.trim_end_matches('/'));
    std::path::Path::new(&abs)
        .exists()
        .then(|| format!("file://{abs}"))
}

/// `src/x.rs:12:5` → `src/x.rs` (the URI's extent; display keeps all).
fn strip_line_anchor(core: &str) -> &str {
    let mut s = core;
    for _ in 0..2 {
        if let Some(colon) = s.rfind(':') {
            let tail = &s[colon + 1..];
            if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) {
                s = &s[..colon];
                continue;
            }
        }
        break;
    }
    s
}

/// Print `line` at `p` in one ink, attaching hyperlinks to its URL and
/// path tokens. Drop-in for the plain `canvas.print` in a draw closure:
/// same cells, same inks, plus OSC-8 targets. Returns the width
/// advanced, like `print`.
pub fn print_linked(
    canvas: &mut dyn StyledCanvas,
    p: Point,
    line: &str,
    ink: Rgba,
    root: Option<&Rc<str>>,
) -> i32 {
    let mut x = p.x;
    for seg in segments(line, root.map(|r| r.as_ref())) {
        match seg {
            Seg::Plain(s) => {
                x += canvas.print(Point::new(x, p.y), &s, ink, Rgba::TRANSPARENT);
            }
            Seg::Link { text, uri } => {
                let id = canvas.register_link(&uri);
                let style = Style::new().fg(ink).bg(Rgba::TRANSPARENT).link(id);
                x += canvas.print_styled(Point::new(x, p.y), &text, &style);
            }
        }
    }
    x - p.x
}

/// The first path-shaped token of `text`, as DISPLAYED (line anchor
/// kept) — the context menu's "Copy path" source. URLs excluded: they
/// have their own affordance.
pub fn first_path(text: &str, root: Option<&str>) -> Option<String> {
    for tok in text.split_whitespace() {
        if let Some((lead, len, uri)) = classify(tok, root) {
            if uri.starts_with("file://") {
                return Some(tok[lead..lead + len].to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link_of(line: &str, root: Option<&str>) -> Option<(String, String)> {
        segments(line, root).into_iter().find_map(|s| match s {
            Seg::Link { text, uri } => Some((text, uri)),
            Seg::Plain(_) => None,
        })
    }

    /// The whole line survives segmentation byte-for-byte: links change
    /// TARGETS, never cells.
    #[test]
    fn segmentation_is_lossless() {
        for line in [
            "cargo test --workspace",
            "read src/ui/mod.rs start_line=1",
            "see (https://docs.rs/abstracttui), then /tmp/x.txt.",
            "   leading and trailing   ",
            "",
        ] {
            let joined: String = segments(line, None)
                .iter()
                .map(|s| match s {
                    Seg::Plain(t) => t.as_str(),
                    Seg::Link { text, .. } => text.as_str(),
                })
                .collect();
            assert_eq!(joined, line);
        }
    }

    #[test]
    fn urls_link_as_themselves_with_punctuation_shed() {
        let (text, uri) = link_of("see https://docs.rs/abstracttui).", None).unwrap();
        assert_eq!(text, "https://docs.rs/abstracttui");
        assert_eq!(uri, "https://docs.rs/abstracttui");
        assert!(link_of("https:// alone", None).is_none(), "scheme-only");
    }

    #[test]
    fn absolute_paths_link_and_line_anchors_stay_in_the_text() {
        let (text, uri) = link_of("edit /tmp/notes.md:12:5 now", None).unwrap();
        assert_eq!(text, "/tmp/notes.md:12:5", "display keeps the anchor");
        assert_eq!(uri, "file:///tmp/notes.md", "the URI sheds it");
    }

    /// A relative path links ONLY against a root it actually exists
    /// under — a dud `file://` to a gateway-side path is a fabricated
    /// affordance, not a link.
    #[test]
    fn relative_paths_require_existence_under_the_root() {
        let dir = std::env::temp_dir().join("acode_linkify_test");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/real.rs"), b"x").unwrap();
        let root = dir.to_string_lossy().to_string();
        let (_, uri) = link_of("open src/real.rs please", Some(&root)).unwrap();
        assert_eq!(uri, format!("file://{root}/src/real.rs"));
        assert!(
            link_of("open src/ghost.rs please", Some(&root)).is_none(),
            "a nonexistent relative path never links"
        );
        assert!(
            link_of("open src/real.rs please", None).is_none(),
            "no root, no relative link"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Argument-shaped tokens never link: flags, key=value, versions.
    #[test]
    fn non_paths_stay_plain() {
        for line in [
            "--features=a/b",
            "-p/x",
            "ratio 3/4 done",
            "a=b/c",
            "just words here",
        ] {
            // `ratio 3/4` DOES look path-shaped; it only links if a file
            // named `3/4` exists under the root — which is the existence
            // rule doing its job, so probe it rootless.
            assert!(link_of(line, None).is_none(), "{line:?}");
        }
    }

    #[test]
    fn first_path_feeds_the_copy_action() {
        assert_eq!(
            first_path("cat /var/log/x.log | head", None).as_deref(),
            Some("/var/log/x.log")
        );
        assert_eq!(first_path("see https://x.dev only", None), None);
    }
}
