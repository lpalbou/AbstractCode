//! The transcript's right-click menu (operator ask, 2026-08-28): a
//! secondary press on a transcript item opens the engine's
//! [`ContextMenu`] with the actions that item actually affords — copy
//! its text, copy a tool call's arguments/result/error/path, quote a
//! message into the composer. Keyboard inside the open menu is the
//! engine's (arrows, Enter, Escape).
//!
//! Split pure/effectful on purpose: [`items_for`] and [`payload`]
//! compute WHAT the menu offers and WHAT an action yields, and are
//! unit-tested; [`act`] performs the one clipboard write or composer
//! seed. Actions are stable string keys (the engine's own contract).

use abstracttui::app::ContextMenuItem;

use crate::store::Store;
use crate::transcript::Item;

/// Nothing a clipboard write would carry. THE gate for every offer and
/// payload here, and it must stay `trim()`-based: the engine's
/// `copy_to_clipboard` REFUSES all-whitespace text (an empty OSC 52
/// payload clears the clipboard, so silence is the right refusal there)
/// — an `is_empty()` gate here offered "Copy result" for a `"\n"` stdout
/// (`mkdir -p out`, `touch x`), dropped the write, and still announced
/// "copied result (1 chars)". A notice for a copy that did not happen is
/// exactly the fabrication ADR 0001 forbids (adversarial review
/// 2026-08-28, I-1).
fn blank(s: &str) -> bool {
    s.trim().is_empty()
}

/// The menu for one transcript item — empty means "no menu opens"
/// (nothing to act on is silence, not a menu of disabled rows).
pub fn items_for(item: &Item, root: Option<&str>) -> Vec<ContextMenuItem> {
    let mut out = Vec::new();
    // Every row's enablement mirrors `payload`'s own gate, so an
    // ENABLED row can never resolve to nothing (that pairing is what
    // keeps `act`'s notice honest — see `blank`).
    match item {
        Item::User { text } | Item::Steer { text } => {
            out.push(ContextMenuItem::new("copy", "Copy message").disabled(blank(text)));
            out.push(ContextMenuItem::new("quote", "Quote in composer").disabled(blank(text)));
        }
        Item::Assistant { text, .. } => {
            out.push(ContextMenuItem::new("copy", "Copy answer").disabled(blank(text)));
            out.push(ContextMenuItem::new("quote", "Quote in composer").disabled(blank(text)));
        }
        Item::Thinking {
            content, reasoning, ..
        } => {
            out.push(
                ContextMenuItem::new("copy", "Copy thinking")
                    .disabled(blank(content) && blank(reasoning)),
            );
        }
        Item::Tool {
            args_preview,
            args_full,
            result,
            error,
            ..
        } => {
            out.push(
                ContextMenuItem::new("copy-args", "Copy arguments")
                    .disabled(blank(args_full) && blank(args_preview)),
            );
            out.push(ContextMenuItem::new("copy-result", "Copy result").disabled(blank(result)));
            if !blank(error) {
                out.push(ContextMenuItem::new("copy-error", "Copy error"));
            }
            let args = if args_full.is_empty() {
                args_preview
            } else {
                args_full
            };
            if let Some(path) = crate::ui::linkify::first_path(args, root) {
                out.push(ContextMenuItem::new("copy-path", "Copy path").hint(path));
            }
        }
        Item::Info { text } | Item::Error { text } => {
            out.push(ContextMenuItem::new("copy", "Copy line").disabled(blank(text)));
        }
        Item::Image { artifact_id, .. } => {
            out.push(ContextMenuItem::new("copy", "Copy artifact id").disabled(blank(artifact_id)));
        }
        Item::Probe { title, body } => {
            out.push(
                ContextMenuItem::new("copy", "Copy probe").disabled(blank(title) && blank(body)),
            );
        }
    }
    out
}

/// What `key` yields for `item`: `(what-it-was, the text)`. `None` =
/// the key does not apply (a stale menu against a mutated fold slot —
/// acting on nothing beats acting on the wrong thing).
pub fn payload(item: &Item, key: &str, root: Option<&str>) -> Option<(&'static str, String)> {
    match (item, key) {
        (Item::User { text } | Item::Steer { text }, "copy" | "quote") => {
            (!blank(text)).then(|| ("message", text.clone()))
        }
        (Item::Assistant { text, .. }, "copy" | "quote") => {
            (!blank(text)).then(|| ("answer", text.clone()))
        }
        (
            Item::Thinking {
                content, reasoning, ..
            },
            "copy",
        ) => {
            let mut body = content.trim().to_string();
            if !reasoning.trim().is_empty() {
                if !body.is_empty() {
                    body.push_str("\n— reasoning —\n");
                }
                body.push_str(reasoning.trim());
            }
            (!blank(&body)).then_some(("thinking", body))
        }
        (
            Item::Tool {
                args_preview,
                args_full,
                ..
            },
            "copy-args",
        ) => {
            let args = if args_full.is_empty() {
                args_preview
            } else {
                args_full
            };
            (!blank(args)).then(|| ("arguments", args.clone()))
        }
        (Item::Tool { result, .. }, "copy-result") => {
            (!blank(result)).then(|| ("result", result.clone()))
        }
        (Item::Tool { error, .. }, "copy-error") => {
            (!blank(error)).then(|| ("error", error.clone()))
        }
        (
            Item::Tool {
                args_preview,
                args_full,
                ..
            },
            "copy-path",
        ) => {
            let args = if args_full.is_empty() {
                args_preview
            } else {
                args_full
            };
            crate::ui::linkify::first_path(args, root).map(|p| ("path", p))
        }
        (Item::Info { text } | Item::Error { text }, "copy") => {
            (!blank(text)).then(|| ("line", text.clone()))
        }
        (Item::Image { artifact_id, .. }, "copy") => {
            (!blank(artifact_id)).then(|| ("artifact id", artifact_id.clone()))
        }
        (Item::Probe { title, body }, "copy") => {
            let text = format!("{title}\n{body}");
            (!blank(&text)).then_some(("probe", text))
        }
        _ => None,
    }
}

/// Perform `key` against `item`: one clipboard write (OSC 52, engine
/// custody) with a notice naming what left, or the composer quote seed.
/// The seed REPLACES the draft — that is `composer_seed`'s documented
/// contract, and the gesture is explicit.
pub fn act(store: Store, item: &Item, key: &str, root: Option<&str>) {
    let Some((what, text)) = payload(item, key, root) else {
        return;
    };
    if key == "quote" {
        let quoted: String = text
            .lines()
            .map(|l| format!("> {l}\n"))
            .chain(std::iter::once("\n".to_string()))
            .collect();
        store.composer_seed.set(Some(quoted));
        store.notify("quoted into the composer");
        return;
    }
    let chars = text.chars().count();
    abstracttui::app::selection::copy_to_clipboard(text);
    store.notify(format!("copied {what} ({chars} chars) — OSC 52 clipboard"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(args_full: &str, result: &str, error: &str) -> Item {
        Item::Tool {
            key: "k".into(),
            name: "execute_command".into(),
            args_preview: "preview".into(),
            args_full: args_full.into(),
            status: crate::transcript::ToolStatus::Ok,
            result: result.into(),
            error: error.into(),
        }
    }

    /// The menu offers exactly what the item affords: no error row
    /// without an error, no path row without a path, result disabled
    /// when empty (visible-but-inert says "there was no output").
    #[test]
    fn tool_menus_match_the_card() {
        let items = items_for(&tool("cat /tmp/a.log", "", ""), None);
        let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, ["copy-args", "copy-result", "copy-path"]);
        assert!(items[1].disabled, "empty result is inert");
        assert_eq!(items[2].hint.as_deref(), Some("/tmp/a.log"));

        let keys: Vec<String> = items_for(&tool("plain words", "out", "boom"), None)
            .iter()
            .map(|i| i.key.clone())
            .collect();
        assert_eq!(keys, ["copy-args", "copy-result", "copy-error"]);
    }

    #[test]
    fn payloads_answer_their_keys_and_refuse_the_rest() {
        let t = tool("cargo test", "42 passed", "");
        assert_eq!(
            payload(&t, "copy-args", None),
            Some(("arguments", "cargo test".into()))
        );
        assert_eq!(
            payload(&t, "copy-result", None),
            Some(("result", "42 passed".into()))
        );
        assert_eq!(payload(&t, "copy-error", None), None, "no error to copy");
        assert_eq!(payload(&t, "quote", None), None, "tools do not quote");
        let user = Item::User { text: "hi".into() };
        assert_eq!(payload(&user, "copy", None), Some(("message", "hi".into())));
    }

    /// WHITESPACE-ONLY content is nothing to copy.
    ///
    /// The engine's `copy_to_clipboard` refuses all-whitespace text (an
    /// empty OSC 52 payload CLEARS the clipboard), so an `is_empty()`
    /// gate here offered "Copy result" for a `"\n"` stdout — `mkdir -p
    /// out`, `touch x`, any command whose output is a bare newline —
    /// then dropped the write and still announced "copied result (1
    /// chars)". A notice for a copy that did not happen is the
    /// fabrication ADR 0001 forbids. Both halves are pinned: the row is
    /// INERT, and the payload is None, so `act` can never reach the
    /// notice (adversarial review 2026-08-28, I-1).
    #[test]
    fn whitespace_only_content_is_nothing_to_copy() {
        let t = tool("cargo build", "\n", "");
        let row = items_for(&t, None)
            .into_iter()
            .find(|i| i.key == "copy-result")
            .expect("the row exists");
        assert!(
            row.disabled,
            "a whitespace-only result is inert, not copyable"
        );
        assert_eq!(
            payload(&t, "copy-result", None),
            None,
            "and yields nothing, so no notice can claim a copy"
        );
        // Same rule on every other lane that reaches the clipboard.
        for item in [
            Item::User { text: "  ".into() },
            Item::Info {
                text: "\t\n".into(),
            },
            Item::Assistant {
                text: " ".into(),
                final_answer: true,
            },
        ] {
            assert_eq!(payload(&item, "copy", None), None, "{item:?}");
            assert!(
                items_for(&item, None).iter().all(|i| i.disabled),
                "every offered row is inert: {item:?}"
            );
        }
        // The control: real content still copies.
        assert!(payload(&tool("x", "real output", ""), "copy-result", None).is_some());
    }

    /// `args_full` empty falls back to the preview — details must never
    /// offer LESS than the folded row (the card's own rule).
    #[test]
    fn args_fall_back_to_the_preview() {
        let t = tool("", "", "");
        assert_eq!(
            payload(&t, "copy-args", None),
            Some(("arguments", "preview".into()))
        );
    }
}
