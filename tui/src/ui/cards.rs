//! Wrapping cards: the list primitive of the Code WUI's sidebar and rail
//! panels, in the terminal.
//!
//! A [`Card`] is a few lines (a title, the quiet facts under it) with an
//! optional right-aligned part per line (the `[x] Active` switch, a badge,
//! a value). Lines WRAP to the width the layout actually grants at draw
//! time — nothing is cut with an ellipsis, so a long title or a long value
//! is always readable in full (R7 convention: wrapping rows, never a silent
//! truncation). The selected card is highlighted on every one of its lines
//! and scrolled fully into view; rows hidden above/below say so ("↑ 3
//! more").
//!
//! Pure layout lives in [`layout`] (test-pinned); [`draw_cards`] only
//! paints what it returns.

use abstracttui::prelude::*;

/// How a line is inked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// Body text.
    Text,
    /// Quiet facts (meta lines, notes).
    Faint,
    /// A heading or a title (accent, bold).
    Title,
    /// An "on" state or a call to action (accent).
    Accent,
    /// An error sentence.
    Error,
    /// A switch that is ON: accent + bold (OFF is `Text`, unavailable is
    /// `Faint` — the state-toggles contract).
    On,
}

/// One logical line of a card: `text` wraps; `right` stays right-aligned on
/// the line's first row (or gets its own row when the width is too small).
#[derive(Clone, Debug, PartialEq)]
pub struct CardLine {
    pub text: String,
    pub right: String,
    pub ink: Ink,
    /// The right part's own ink (a switch's state); `None` = the line's.
    pub right_ink: Option<Ink>,
    /// Extra indent of this line (and its continuation rows), in cells.
    pub indent: usize,
}

impl CardLine {
    pub fn new(text: impl Into<String>, ink: Ink) -> CardLine {
        CardLine {
            text: text.into(),
            right: String::new(),
            ink,
            right_ink: None,
            indent: 0,
        }
    }

    pub fn right_ink(mut self, ink: Ink) -> CardLine {
        self.right_ink = Some(ink);
        self
    }

    pub fn right(mut self, right: impl Into<String>) -> CardLine {
        self.right = right.into();
        self
    }

    pub fn indent(mut self, n: usize) -> CardLine {
        self.indent = n;
        self
    }
}

/// A card: its lines and whether the cursor can land on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub lines: Vec<CardLine>,
    pub selectable: bool,
}

impl Card {
    pub fn new(lines: Vec<CardLine>) -> Card {
        Card {
            lines,
            selectable: true,
        }
    }

    /// A card the cursor skips (a heading, a note, a blank separator).
    pub fn fixed(lines: Vec<CardLine>) -> Card {
        Card {
            lines,
            selectable: false,
        }
    }

    pub fn note(text: impl Into<String>) -> Card {
        Card::fixed(vec![CardLine::new(text, Ink::Faint)])
    }

    pub fn heading(text: impl Into<String>) -> Card {
        Card::fixed(vec![CardLine::new(text, Ink::Title)])
    }
}

/// One painted row.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub card: usize,
    pub text: String,
    pub ink: Ink,
    /// Where the right part starts in `text` (byte index) and its ink.
    pub right_at: Option<(usize, Ink)>,
}

fn width_of(s: &str) -> usize {
    abstracttui::text::width(s).max(0) as usize
}

/// Wrap `text` to `width` cells (words kept whole where possible).
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(4);
    let mut out = Vec::new();
    for raw in text.split('\n') {
        if raw.trim().is_empty() {
            out.push(String::new());
            continue;
        }
        out.extend(abstracttui::text::wrap(raw, width as i32));
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Lay the cards out at `width` cells: every row, tagged with its card.
/// Column 0–1 hold the cursor marker ("▸ "), so text starts at column 2.
pub fn layout(cards: &[Card], width: usize) -> Vec<Row> {
    let inner = width.saturating_sub(2).max(8);
    let mut rows = Vec::new();
    for (ci, card) in cards.iter().enumerate() {
        for line in &card.lines {
            let indent = line.indent.min(inner / 2);
            let avail = inner - indent;
            let pad = " ".repeat(indent);
            let rw = width_of(&line.right);
            if rw == 0 {
                for w in wrap(&line.text, avail) {
                    rows.push(Row {
                        card: ci,
                        text: format!("{pad}{w}"),
                        ink: line.ink,
                        right_at: None,
                    });
                }
                continue;
            }
            // The right part shares the row only when the WHOLE text fits
            // beside it; otherwise the text keeps the full width and the
            // right part gets its own row (a label is never squeezed into
            // a one-word column).
            if rw + 10 > avail || width_of(&line.text) + 2 + rw > avail {
                // Too narrow to share the row: text, then the right part
                // right-aligned on its own row (never dropped).
                if !line.text.is_empty() {
                    for w in wrap(&line.text, avail) {
                        rows.push(Row {
                            card: ci,
                            text: format!("{pad}{w}"),
                            ink: line.ink,
                            right_at: None,
                        });
                    }
                }
                for w in wrap(&line.right, avail) {
                    let gap = avail.saturating_sub(width_of(&w));
                    let head = format!("{pad}{}", " ".repeat(gap));
                    rows.push(Row {
                        card: ci,
                        right_at: line.right_ink.map(|i| (head.len(), i)),
                        text: format!("{head}{w}"),
                        ink: line.ink,
                    });
                }
                continue;
            }
            let wrapped = if line.text.is_empty() {
                vec![String::new()]
            } else {
                wrap(&line.text, avail - rw - 2)
            };
            for (i, w) in wrapped.into_iter().enumerate() {
                let (text, right_at) = if i == 0 {
                    let gap = avail.saturating_sub(width_of(&w) + rw).max(2);
                    let head = format!("{pad}{w}{}", " ".repeat(gap));
                    let at = line.right_ink.map(|ink| (head.len(), ink));
                    (format!("{head}{}", line.right), at)
                } else {
                    (format!("{pad}{w}"), None)
                };
                rows.push(Row {
                    card: ci,
                    text,
                    ink: line.ink,
                    right_at,
                });
            }
        }
    }
    rows
}

/// The card index of the `cursor`-th selectable card.
pub fn selected_card(cards: &[Card], cursor: usize) -> Option<usize> {
    cards
        .iter()
        .enumerate()
        .filter(|(_, c)| c.selectable)
        .nth(cursor)
        .map(|(i, _)| i)
}

/// First visible row so that the selected card is fully in view (or its
/// top, when it is taller than the window).
pub fn window_start(rows: &[Row], selected: Option<usize>, height: usize) -> usize {
    if rows.len() <= height {
        return 0;
    }
    let Some(card) = selected else { return 0 };
    let first = rows.iter().position(|r| r.card == card).unwrap_or(0);
    let last = rows.iter().rposition(|r| r.card == card).unwrap_or(first);
    let max_start = rows.len() - height;
    // Keep one row of context above the card where possible.
    if last + 2 <= height {
        0
    } else if last - first + 1 >= height {
        first.min(max_start)
    } else {
        (last + 2 - height).min(max_start).min(first)
    }
}

/// Paint `cards` with the `cursor`-th selectable card highlighted.
pub fn draw_cards(cards: Vec<Card>, cursor: usize) -> View {
    let t = abstracttui::app::current_theme().tokens;
    let selected = selected_card(&cards, cursor);
    Element::new()
        .style(LayoutStyle::column().grow(1.0).basis(Dimension::Cells(0)))
        .draw(move |canvas, rect| {
            let rows = layout(&cards, rect.w.max(10) as usize);
            let h = rect.h.max(1) as usize;
            let start = window_start(&rows, selected, h);
            let shown = h.min(rows.len() - start.min(rows.len()));
            let cut_above = start;
            let cut_below = rows.len().saturating_sub(start + shown);
            for line in 0..shown {
                let row = &rows[start + line];
                let y = rect.y + line as i32;
                let note = if line == 0 && cut_above > 0 {
                    Some(format!("↑ {cut_above} more"))
                } else if line + 1 == shown && cut_below > 0 {
                    Some(format!("↓ {cut_below} more"))
                } else {
                    None
                };
                if let Some(msg) = note {
                    canvas.print(
                        Point::new(rect.x + 2, y),
                        &msg,
                        t.text_faint,
                        Rgba::TRANSPARENT,
                    );
                    continue;
                }
                let is_sel = selected == Some(row.card);
                let bg = if is_sel {
                    t.selection_bg
                } else {
                    Rgba::TRANSPARENT
                };
                if is_sel {
                    canvas.fill(Rect::new(rect.x, y, rect.w, 1), ' ', t.selection_fg, bg);
                }
                let first_of_card = start + line == 0 || rows[start + line - 1].card != row.card;
                let marker = if is_sel && first_of_card {
                    "▸ "
                } else {
                    "  "
                };
                let style_of = |ink: Ink| {
                    let fg = if is_sel {
                        t.selection_fg
                    } else {
                        match ink {
                            Ink::Text => t.text,
                            Ink::Faint => t.text_faint,
                            Ink::Title | Ink::Accent | Ink::On => t.accent,
                            Ink::Error => t.error,
                        }
                    };
                    let mut style = abstracttui::render::Style::new().fg(fg).bg(bg);
                    if matches!(ink, Ink::Title | Ink::On) {
                        style = style.attrs(abstracttui::render::Attrs::BOLD);
                    }
                    style
                };
                let (left, right) = match row.right_at {
                    Some((at, ink)) if at <= row.text.len() => {
                        (&row.text[..at], Some((&row.text[at..], ink)))
                    }
                    _ => (row.text.as_str(), None),
                };
                let head = format!("{marker}{left}");
                canvas.print_styled(Point::new(rect.x, y), &head, &style_of(row.ink));
                if let Some((r, ink)) = right {
                    canvas.print_styled(
                        Point::new(rect.x + width_of(&head) as i32, y),
                        r,
                        &style_of(ink),
                    );
                }
            }
        })
        .build()
}

/// A card list's scroll position that outlives a repaint: `top` = the
/// first painted row (written by the paint, moved by the wheel), `follow`
/// = keep the cursor's card in view (keys) or not (the wheel scrolls
/// freely), `height` = the last painted height (page keys), `tick`
/// repaints after a wheel.
#[derive(Clone)]
pub struct CardScroll {
    pub top: std::rc::Rc<std::cell::Cell<usize>>,
    pub follow: std::rc::Rc<std::cell::Cell<bool>>,
    pub height: std::rc::Rc<std::cell::Cell<usize>>,
    pub tick: Signal<u64>,
}

impl CardScroll {
    pub fn new(cx: Scope) -> CardScroll {
        CardScroll {
            top: Default::default(),
            follow: std::rc::Rc::new(std::cell::Cell::new(true)),
            height: std::rc::Rc::new(std::cell::Cell::new(10)),
            tick: cx.signal(0u64),
        }
    }

    /// The mouse wheel: three rows, the cursor stays where it is.
    pub fn wheel(&self, up: bool) {
        self.follow.set(false);
        let top = self.top.get();
        self.top
            .set(if up { top.saturating_sub(3) } else { top + 3 });
        self.tick.update(|n| *n += 1);
    }

    /// A key moved the cursor: the window follows it again.
    pub fn follow_cursor(&self) {
        self.follow.set(true);
    }

    /// Back to the top (another panel opened).
    pub fn reset(&self) {
        self.top.set(0);
        self.follow.set(true);
    }

    /// Cards a page key moves the cursor by.
    pub fn page(&self) -> i64 {
        (self.height.get() / 3).max(1) as i64
    }
}

/// Pure: the first visible row of a scrolled card list. Following the
/// cursor, the FIRST selectable card shows the list's top (the headings
/// above it), the LAST shows its end (the notes under it — nothing is ever
/// stranded below the last row you can select), any other card is scrolled
/// into view with one row of context; not following (the wheel), `top`
/// stays, clamped.
pub fn scroll_window(
    cards: &[Card],
    rows: &[Row],
    cursor: usize,
    top: usize,
    height: usize,
    follow: bool,
) -> usize {
    let max_start = rows.len().saturating_sub(height);
    let top = top.min(max_start);
    if !follow || rows.len() <= height {
        return if rows.len() <= height { 0 } else { top };
    }
    let n_sel = cards.iter().filter(|c| c.selectable).count();
    let Some(card) = selected_card(cards, cursor) else {
        return top;
    };
    let first = rows.iter().position(|r| r.card == card).unwrap_or(0);
    let last = rows.iter().rposition(|r| r.card == card).unwrap_or(first);
    // The ends, when the cursor's card is still whole in that window.
    if cursor == 0 && last < height {
        return 0;
    }
    if n_sel > 0 && cursor + 1 >= n_sel && first >= max_start {
        return max_start;
    }
    if first < top + 1 {
        first.saturating_sub(1).min(max_start)
    } else if last + 2 > top + height {
        (last + 2).saturating_sub(height).min(first).min(max_start)
    } else {
        top
    }
}

/// [`draw_cards`] with a [`CardScroll`]: the wheel scrolls, the cursor's
/// card stays in view while keys move it, and the counts of rows above /
/// below are drawn at the right edge (no row is given up for them).
pub fn draw_cards_scrolled(cards: Vec<Card>, cursor: usize, scroll: CardScroll) -> View {
    let t = abstracttui::app::current_theme().tokens;
    let _ = scroll.tick.get();
    let selected = selected_card(&cards, cursor);
    Element::new()
        .style(LayoutStyle::column().grow(1.0).basis(Dimension::Cells(0)))
        .draw(move |canvas, rect| {
            let rows = layout(&cards, rect.w.max(10) as usize);
            let h = rect.h.max(1) as usize;
            scroll.height.set(h);
            let start = scroll_window(
                &cards,
                &rows,
                cursor,
                scroll.top.get(),
                h,
                scroll.follow.get(),
            );
            scroll.top.set(start);
            for (line, ri) in (start..rows.len()).take(h).enumerate() {
                let row = &rows[ri];
                let y = rect.y + line as i32;
                let is_sel = selected == Some(row.card);
                let bg = if is_sel {
                    t.selection_bg
                } else {
                    Rgba::TRANSPARENT
                };
                if is_sel {
                    canvas.fill(Rect::new(rect.x, y, rect.w, 1), ' ', t.selection_fg, bg);
                }
                let first_of_card = ri == 0 || rows[ri - 1].card != row.card;
                let marker = if is_sel && first_of_card {
                    "▸ "
                } else {
                    "  "
                };
                let style_of = |ink: Ink| {
                    let fg = if is_sel {
                        t.selection_fg
                    } else {
                        match ink {
                            Ink::Text => t.text,
                            Ink::Faint => t.text_faint,
                            Ink::Title | Ink::Accent | Ink::On => t.accent,
                            Ink::Error => t.error,
                        }
                    };
                    let mut style = abstracttui::render::Style::new().fg(fg).bg(bg);
                    if matches!(ink, Ink::Title | Ink::On) {
                        style = style.attrs(abstracttui::render::Attrs::BOLD);
                    }
                    style
                };
                let (left, right) = match row.right_at {
                    Some((at, ink)) if at <= row.text.len() => {
                        (&row.text[..at], Some((&row.text[at..], ink)))
                    }
                    _ => (row.text.as_str(), None),
                };
                let head = format!("{marker}{left}");
                canvas.print_styled(Point::new(rect.x, y), &head, &style_of(row.ink));
                if let Some((r, ink)) = right {
                    canvas.print_styled(
                        Point::new(rect.x + width_of(&head) as i32, y),
                        r,
                        &style_of(ink),
                    );
                }
            }
            let below = rows.len().saturating_sub(start + h);
            for (n, y, arrow) in [(start, rect.y, "↑"), (below, rect.bottom() - 1, "↓")] {
                if n > 0 {
                    let msg = format!(" {arrow} {n} more ");
                    let w = abstracttui::text::width(&msg);
                    let at = Point::new(rect.x + rect.w - w, y);
                    canvas.fill(
                        Rect::new(at.x, y, w, 1),
                        ' ',
                        t.text_faint,
                        Rgba::TRANSPARENT,
                    );
                    canvas.print(at, &msg, t.text_faint, Rgba::TRANSPARENT);
                }
            }
        })
        .build()
}

/// Faint sentences wrapped to the current viewport (`inset` = the panel's
/// horizontal chrome) — status, errors and notes above or below a list,
/// never cut with an ellipsis.
pub fn note_lines(t: &TokenSet, lines: &[String], inset: i32) -> View {
    let width = (abstracttui::app::current_viewport().w - inset).max(20) as usize;
    let rows: Vec<String> = lines
        .iter()
        .filter(|l| !l.is_empty())
        .flat_map(|l| wrap(l, width))
        .collect();
    let faint = t.text_faint;
    let n = rows.len() as i32;
    Element::new()
        .style(LayoutStyle::line(n).shrink(0.0))
        .draw(move |canvas, rect| {
            for (i, l) in rows.iter().enumerate() {
                canvas.print(
                    Point::new(rect.x, rect.y + i as i32),
                    l,
                    faint,
                    Rgba::TRANSPARENT,
                );
            }
        })
        .build()
}

/// Key hints as `key label` pairs joined by " · ", split into as many rows
/// as `width` needs — whole pairs only, never an ellipsis.
pub fn hint_lines(pairs: &[(&str, &str)], width: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for (k, l) in pairs {
        let item = match (k.is_empty(), l.is_empty()) {
            (true, _) => l.to_string(),
            (false, true) => k.to_string(),
            _ => format!("{k} {l}"),
        };
        let next = if cur.is_empty() {
            item.clone()
        } else {
            format!("{cur} · {item}")
        };
        if width_of(&next) > width && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            cur = item;
        } else {
            cur = next;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The key-hint bar: faint rows at the bottom of a screen, wrapped to the
/// current viewport (`inset` = the panel's horizontal chrome).
pub fn hint_bar(t: &TokenSet, pairs: &[(&str, &str)], inset: i32) -> View {
    let width = (abstracttui::app::current_viewport().w - inset).max(20) as usize;
    let lines = hint_lines(pairs, width);
    let faint = t.text_faint;
    let n = lines.len().max(1) as i32;
    Element::new()
        .style(LayoutStyle::line(n).shrink(0.0))
        .draw(move |canvas, rect| {
            for (i, l) in lines.iter().enumerate() {
                canvas.print(
                    Point::new(rect.x, rect.y + i as i32),
                    l,
                    faint,
                    Rgba::TRANSPARENT,
                );
            }
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_text_shares_its_row_with_the_right_part() {
        let cards = vec![Card::new(vec![
            CardLine::new("next in 20 h", Ink::Faint).right("[x] Active")
        ])];
        let rows = layout(&cards, 40);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].text.starts_with("next in 20 h"));
        assert!(rows[0].text.ends_with("[x] Active"), "{rows:?}");
    }

    #[test]
    fn a_long_text_keeps_its_width_and_the_right_part_gets_its_own_row() {
        let cards = vec![Card::new(vec![CardLine::new(
            "a title long enough to need two rows at this width",
            Ink::Title,
        )
        .right("[x] Active")])];
        let rows = layout(&cards, 40);
        assert!(rows.len() >= 3, "{rows:?}");
        assert!(
            rows.last().unwrap().text.trim_start() == "[x] Active",
            "{rows:?}"
        );
        let all: String = rows
            .iter()
            .map(|r| r.text.trim().to_string() + " ")
            .collect();
        assert!(
            all.contains("need two rows at this width"),
            "nothing cut: {all}"
        );
    }

    #[test]
    fn a_narrow_width_moves_the_right_part_to_its_own_row() {
        let cards = vec![Card::new(vec![
            CardLine::new("next in 20 h", Ink::Faint).right("[x] Active")
        ])];
        let rows = layout(&cards, 18);
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert!(rows[1].text.trim_end().ends_with("[x] Active"));
    }

    #[test]
    fn the_window_keeps_the_selected_card_in_view() {
        let cards: Vec<Card> = (0..20)
            .map(|i| {
                Card::new(vec![
                    CardLine::new(format!("card {i}"), Ink::Title),
                    CardLine::new("meta", Ink::Faint),
                ])
            })
            .collect();
        let rows = layout(&cards, 40);
        let start = window_start(&rows, Some(15), 10);
        let visible: Vec<_> = rows[start..start + 10].iter().map(|r| r.card).collect();
        assert!(
            visible.iter().filter(|c| **c == 15).count() == 2,
            "{visible:?}"
        );
    }

    #[test]
    fn a_scrolled_list_never_strands_rows_above_or_below() {
        // A heading, 12 selectable cards, then two trailing notes.
        let mut cards = vec![Card::heading("Head")];
        cards.extend((0..12).map(|i| Card::new(vec![CardLine::new(format!("c{i}"), Ink::Text)])));
        cards.push(Card::note("note one"));
        cards.push(Card::note("note two"));
        let rows = layout(&cards, 40);
        assert_eq!(rows.len(), 15);
        // First selectable: the heading shows.
        assert_eq!(scroll_window(&cards, &rows, 0, 7, 6, true), 0);
        // Last selectable: the notes under it show (the end).
        assert_eq!(scroll_window(&cards, &rows, 11, 0, 6, true), 9);
        // A first card below a tall preamble is still brought into view.
        let mut tall = vec![Card::fixed(
            (0..8)
                .map(|i| CardLine::new(format!("p{i}"), Ink::Faint))
                .collect(),
        )];
        tall.push(Card::new(vec![CardLine::new("first", Ink::Text)]));
        let trows = layout(&tall, 40);
        let start = scroll_window(&tall, &trows, 0, 0, 5, true);
        assert!(start + 5 > 8, "{start}");
        // A middle card comes into view with a row of context.
        let start = scroll_window(&cards, &rows, 6, 0, 6, true);
        assert!(start <= 7 && start + 6 > 7, "{start}");
        // The wheel: the top stays (clamped), the cursor does not drag it.
        assert_eq!(scroll_window(&cards, &rows, 0, 4, 6, false), 4);
        assert_eq!(scroll_window(&cards, &rows, 0, 99, 6, false), 9);
    }

    #[test]
    fn hints_wrap_whole_pairs() {
        let lines = hint_lines(&[("↑↓", "move"), ("Enter", "opens"), ("Esc", "closes")], 18);
        assert_eq!(lines, vec!["↑↓ move", "Enter opens", "Esc closes"]);
    }
}
