//! Tool output that IS JSON, rendered as JSON (operator ask 2026-10-10:
//! "json should be nicely formatted") — indented, one member per row,
//! keys / strings / numbers / literals / punctuation in the theme's
//! syntax inks, long strings wrapping inside the transcript width.
//!
//! The text is VALIDATED with serde_json (a body that does not parse as
//! one JSON object or array renders as before, verbatim), then
//! RE-INDENTED from its own tokens rather than re-serialised: serde_json's
//! map here is ordered by key, and a re-serialisation would silently
//! reorder the tool's output (`"success"` would no longer lead) and
//! re-spell its numbers. What the tool wrote is what is shown, laid out.

use abstracttui::prelude::*;

/// Bodies above this size stay verbatim: laying out megabytes per render
/// would cost more than the formatting is worth (the copy/export paths
/// keep the raw text either way).
pub const MAX_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Key,
    Str,
    Num,
    Lit,
    Punct,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Open(char),
    Close(char),
    Colon,
    Comma,
    Str(String),
    Atom(String),
}

/// `Some` when `text` is one JSON object or array (surrounding whitespace
/// allowed) small enough to lay out.
pub fn is_json(text: &str) -> bool {
    let t = text.trim();
    if t.len() > MAX_BYTES || !(t.starts_with('{') || t.starts_with('[')) {
        return false;
    }
    serde_json::from_str::<serde_json::Value>(t).is_ok()
}

fn tokenize(text: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut chars = text.trim().chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '[' => out.push(Tok::Open(c)),
            '}' | ']' => out.push(Tok::Close(c)),
            ':' => out.push(Tok::Colon),
            ',' => out.push(Tok::Comma),
            '"' => {
                let mut s = String::from('"');
                let mut escaped = false;
                for d in chars.by_ref() {
                    s.push(d);
                    if escaped {
                        escaped = false;
                    } else if d == '\\' {
                        escaped = true;
                    } else if d == '"' {
                        break;
                    }
                }
                out.push(Tok::Str(s));
            }
            c if c.is_whitespace() => {}
            _ => {
                let mut s = String::from(c);
                while let Some(&d) = chars.peek() {
                    if d.is_whitespace() || matches!(d, ',' | ':' | '}' | ']') {
                        break;
                    }
                    s.push(d);
                    chars.next();
                }
                out.push(Tok::Atom(s));
            }
        }
    }
    out
}

/// The laid-out rows: each row is its indent plus (text, kind) segments.
/// Empty containers stay on one row (`{}` / `[]`).
pub fn layout(text: &str) -> Vec<Vec<(String, Kind)>> {
    let toks = tokenize(text);
    let mut rows: Vec<Vec<(String, Kind)>> = Vec::new();
    let mut cur: Vec<(String, Kind)> = Vec::new();
    let mut depth = 0usize;
    let indent = |d: usize| ("  ".repeat(d), Kind::Punct);
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Tok::Open(c) => {
                let close = if *c == '{' { '}' } else { ']' };
                if matches!(toks.get(i + 1), Some(Tok::Close(_))) {
                    cur.push((format!("{c}{close}"), Kind::Punct));
                    i += 2;
                    continue;
                }
                cur.push((c.to_string(), Kind::Punct));
                rows.push(std::mem::take(&mut cur));
                depth += 1;
                cur.push(indent(depth));
            }
            Tok::Close(c) => {
                if !cur.iter().all(|(s, _)| s.trim().is_empty()) {
                    rows.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
                depth = depth.saturating_sub(1);
                cur.push(indent(depth));
                cur.push((c.to_string(), Kind::Punct));
            }
            Tok::Colon => cur.push((": ".into(), Kind::Punct)),
            Tok::Comma => {
                cur.push((",".into(), Kind::Punct));
                rows.push(std::mem::take(&mut cur));
                cur.push(indent(depth));
            }
            Tok::Str(s) => {
                let key = matches!(toks.get(i + 1), Some(Tok::Colon));
                cur.push((s.clone(), if key { Kind::Key } else { Kind::Str }));
            }
            Tok::Atom(a) => {
                let kind = if matches!(a.as_str(), "true" | "false" | "null") {
                    Kind::Lit
                } else {
                    Kind::Num
                };
                cur.push((a.clone(), kind));
            }
        }
        i += 1;
    }
    if !cur.iter().all(|(s, _)| s.trim().is_empty()) {
        rows.push(cur);
    }
    rows
}

/// The layout as plain text (the folded tool row's inline body, copy).
pub fn pretty(text: &str) -> String {
    layout(text)
        .iter()
        .map(|row| row.iter().map(|(s, _)| s.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Left margin of the block: the tool card's own body indent.
const MARGIN: i32 = 2;
/// Extra hang for a wrapped continuation of a long value.
const HANG: i32 = 2;

type VisualLine = (i32, Vec<(String, Rgba)>);
/// The wrap of the last width asked for.
type WrapMemo = Option<(i32, std::rc::Rc<Vec<VisualLine>>)>;

/// The laid-out JSON as a feed block: rows keep their indentation (a rich
/// block trims leading spaces when it wraps — measured: every row came
/// out flush left), long values wrap with a hanging indent, the wrap is
/// memoized per width, and off-screen rows are culled (the same rules as
/// the transcript's other custom bodies).
pub fn block(text: &str, t: &TokenSet) -> abstracttui::widgets::FeedBlock {
    use std::cell::RefCell;
    use std::rc::Rc;
    let ink = |k: Kind| match k {
        Kind::Key => t.syntax_func,
        Kind::Str => t.syntax_string,
        Kind::Num => t.syntax_number,
        Kind::Lit => t.syntax_keyword,
        Kind::Punct => t.syntax_punct,
    };
    // (indent columns, segments) per logical row.
    let rows: Vec<(i32, Vec<(String, Rgba)>)> = layout(text)
        .into_iter()
        .map(|row| {
            let mut indent = 0;
            let mut segs = Vec::new();
            for (s, k) in row {
                if segs.is_empty() && k == Kind::Punct && s.trim().is_empty() {
                    indent += abstracttui::text::width(&s);
                } else {
                    segs.push((s, ink(k)));
                }
            }
            (indent, segs)
        })
        .collect();
    let rows = Rc::new(rows);
    let memo: Rc<RefCell<WrapMemo>> = Rc::new(RefCell::new(None));
    let lines_at = {
        let rows = rows.clone();
        let memo = memo.clone();
        move |width: i32| -> Rc<Vec<VisualLine>> {
            if let Some((w, l)) = memo.borrow().as_ref() {
                if *w == width {
                    return l.clone();
                }
            }
            let l = Rc::new(wrap(&rows, width));
            *memo.borrow_mut() = Some((width, l.clone()));
            l
        }
    };
    let h_lines = lines_at.clone();
    abstracttui::widgets::FeedBlock::Custom(abstracttui::widgets::CustomBlock::new(
        move |width| h_lines(width).len() as i32,
        move |canvas, rect| {
            let clip = canvas.clip_rect();
            for (i, (x, segs)) in lines_at(rect.w).iter().enumerate() {
                let y = rect.y + i as i32;
                if y < clip.y || y >= clip.bottom() {
                    continue;
                }
                let mut cx = rect.x + x;
                for (s, ink) in segs {
                    cx += canvas.print(Point::new(cx, y), s, *ink, Rgba::TRANSPARENT);
                }
            }
        },
    ))
}

/// Break the rows at `width`: each visual line is (x offset, segments).
fn wrap(rows: &[(i32, Vec<(String, Rgba)>)], width: i32) -> Vec<VisualLine> {
    let mut out = Vec::new();
    for (indent, segs) in rows {
        let first_x = (MARGIN + indent).min((width - 8).max(0));
        let cont_x = (first_x + HANG).min((width - 8).max(0));
        let mut x = first_x;
        let mut avail = (width - x).max(1);
        let mut line: Vec<(String, Rgba)> = Vec::new();
        let mut used = 0;
        for (s, ink) in segs {
            let mut piece = String::new();
            for ch in s.chars() {
                let w = abstracttui::text::width(ch.encode_utf8(&mut [0u8; 4]));
                if used + w > avail && used > 0 {
                    if !piece.is_empty() {
                        line.push((std::mem::take(&mut piece), *ink));
                    }
                    out.push((x, std::mem::take(&mut line)));
                    x = cont_x;
                    avail = (width - x).max(1);
                    used = 0;
                }
                piece.push(ch);
                used += w;
            }
            if !piece.is_empty() {
                line.push((piece, *ink));
            }
        }
        out.push((x, line));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lays_out_in_the_tools_own_key_order() {
        let raw = r#"{"success":true,"query":"top news","params":{"n":10,"tags":[]},"results":[{"rank":1,"title":"A, B: c"},{"rank":2,"title":"say \"hi\""}],"x":null}"#;
        assert!(is_json(raw));
        let p = pretty(raw);
        assert_eq!(
            p,
            "{\n  \"success\": true,\n  \"query\": \"top news\",\n  \"params\": {\n    \"n\": 10,\n    \"tags\": []\n  },\n  \"results\": [\n    {\n      \"rank\": 1,\n      \"title\": \"A, B: c\"\n    },\n    {\n      \"rank\": 2,\n      \"title\": \"say \\\"hi\\\"\"\n    }\n  ],\n  \"x\": null\n}"
        );
        // The layout re-parses to the same value.
        let a: serde_json::Value = serde_json::from_str(raw).unwrap();
        let b: serde_json::Value = serde_json::from_str(&p).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn keys_strings_numbers_and_literals_are_told_apart() {
        let rows = layout(r#"{"k":"v","n":1.5,"b":false}"#);
        let kinds: Vec<(String, Kind)> = rows
            .into_iter()
            .flatten()
            .filter(|(s, _)| !s.trim().is_empty())
            .collect();
        assert!(kinds.contains(&("\"k\"".into(), Kind::Key)));
        assert!(kinds.contains(&("\"v\"".into(), Kind::Str)));
        assert!(kinds.contains(&("1.5".into(), Kind::Num)));
        assert!(kinds.contains(&("false".into(), Kind::Lit)));
    }

    #[test]
    fn rows_keep_their_indent_and_long_values_hang() {
        let rows: Vec<(i32, Vec<(String, Rgba)>)> = vec![
            (0, vec![("{".into(), Rgba::TRANSPARENT)]),
            (
                2,
                vec![
                    ("\"k\": ".into(), Rgba::TRANSPARENT),
                    ("\"abcdefghijklmnopqrstuvwxyz\"".into(), Rgba::TRANSPARENT),
                ],
            ),
        ];
        let lines = wrap(&rows, 20);
        assert_eq!(lines[0].0, 2, "the block margin");
        assert_eq!(lines[1].0, 4, "the row keeps its indent");
        assert!(lines.len() > 2, "a long value wraps: {lines:?}");
        assert_eq!(lines[2].0, 6, "continuations hang under the value");
        for (x, segs) in &lines {
            let w: i32 = segs.iter().map(|(s, _)| abstracttui::text::width(s)).sum();
            assert!(x + w <= 20, "fits the width: {x} + {w}");
        }
    }

    #[test]
    fn only_whole_objects_and_arrays_qualify() {
        assert!(is_json("  [1, 2]\n"));
        assert!(!is_json("42"));
        assert!(!is_json("\"text\""));
        assert!(!is_json("{not json}"));
        assert!(!is_json("ok: {\"a\":1}"));
        assert!(!is_json(""));
    }
}
