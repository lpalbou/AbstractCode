//! `/animation 3 · drift` — the work's own vocabulary as a field.
//!
//! The operator's idea: extract a THEME from what the agent is working
//! on and draw it. The danger in that idea is precise — a rarity-weighted
//! extractor over a repository surfaces exactly the strings you cannot
//! project in a meeting (a key, a customer name, a comment someone wrote
//! at 2am). So this one is deliberately narrow about where words come
//! from:
//!
//! - the user's OWN brief (they wrote it, they own it) — house red;
//! - file BASENAMES and tool NAMES from tool arguments — house blue;
//! - nothing else. Never a tool result, never model output, never an
//!   error body. Everything passes the feed's charset gate first.
//!
//! What it shows: terms sized by how hot they are RIGHT NOW (a decayed
//! weight, so the field follows the work instead of accumulating), placed
//! at a position hashed from the word itself — so a term always returns
//! to the same spot and reopening the pane is not a reshuffle. Colour is
//! the axis worth having: **red terms are your brief, blue terms are the
//! agent's own trail. A field that has gone entirely blue is an agent
//! that has drifted off what you asked for.**

use abstracttui::base::{Point, Rect, Rgba};
use abstracttui::prelude::*;
use abstracttui::theme::derive::mix;
use abstracttui::ui::StyledCanvas;

use super::{Feed, Snapshot, Term};

/// Terms the field tracks. Past this the coldest is replaced.
const MAX_TERMS: usize = 48;
/// Terms drawn at once (the rest are alive but below the fold).
const DRAWN: usize = 22;

/// Words that say nothing about the work: language glue plus the
/// vocabulary every coding session shares. A term list without this is a
/// list of the word "the".
const STOP: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "you", "your", "our", "from", "into", "then",
    "than", "but", "not", "are", "was", "were", "will", "would", "should", "could", "can", "its",
    "it's", "have", "has", "had", "does", "did", "done", "make", "made", "use", "using", "used",
    "get", "got", "put", "add", "added", "also", "just", "only", "any", "all", "some", "one",
    "two", "new", "old", "please", "let", "fn", "pub", "let's", "self", "impl", "struct", "enum",
    "return", "true", "false", "null", "none", "some", "def", "class", "import", "const", "var",
    "file", "files", "code", "test", "tests", "error", "errors", "data", "type", "value", "name",
    "list", "item", "items", "run", "runs",
];

/// Fold a term into the field. `weight` is how much this sighting counts;
/// `origin` is 0 for the user's brief and 1 for the agent's own trail.
pub fn absorb_term(terms: &mut Vec<Term>, word: &str, weight: f32, origin: f32) {
    let w = word.trim().to_ascii_lowercase();
    if w.len() < 3 || w.len() > 24 || w.chars().all(|c| c.is_ascii_digit()) {
        return;
    }
    if STOP.contains(&w.as_str()) {
        return;
    }
    if let Some(t) = terms.iter_mut().find(|t| t.word == w) {
        t.heat += weight;
        // Origin is a running mix: a word in BOTH your brief and the
        // agent's trail sits in the middle of the ramp, which is exactly
        // what it means.
        t.origin += (origin - t.origin) * 0.15;
        return;
    }
    if terms.len() >= MAX_TERMS {
        // Replace the coldest rather than growing without bound.
        let coldest = terms
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.heat.total_cmp(&b.1.heat))
            .map(|(i, _)| i);
        match coldest {
            Some(i) if terms[i].heat < weight => {
                terms.swap_remove(i);
            }
            _ => return,
        }
    }
    terms.push(Term {
        pos: disc_position(&w),
        word: w,
        heat: weight,
        origin,
    });
}

/// Cool every term. Heat is a decayed weight, so the field shows what the
/// work is about NOW, not what it has ever mentioned.
pub fn decay(terms: &mut Vec<Term>, factor: f32) {
    for t in terms.iter_mut() {
        t.heat *= factor;
    }
    terms.retain(|t| t.heat > 0.05);
}

/// A stable position on the unit disc, hashed from the word: the same
/// term always lands in the same place, for the whole run and across
/// re-openings of the pane.
fn disc_position(word: &str) -> (f32, f32) {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in word.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let a = (h % 10_000) as f32 / 10_000.0 * std::f32::consts::TAU;
    let r = ((h >> 20) % 10_000) as f32 / 10_000.0;
    (a.cos() * r.sqrt(), a.sin() * r.sqrt())
}

pub fn render(
    canvas: &mut dyn StyledCanvas,
    rect: Rect,
    t: &TokenSet,
    feed: &Feed,
    snap: &Snapshot,
    frame: u64,
) {
    if rect.w < 24 || rect.h < 6 {
        return;
    }
    let mut terms: Vec<&Term> = feed.terms.iter().collect();
    terms.sort_by(|a, b| b.heat.total_cmp(&a.heat));
    if terms.is_empty() {
        let msg = "no vocabulary yet — the field fills as the run reads and writes";
        let msg = abstracttui::text::truncate_ellipsis(msg, rect.w - 2);
        canvas.print(
            Point::new(
                rect.x + (rect.w - msg.chars().count() as i32) / 2,
                rect.y + rect.h / 2,
            ),
            &msg,
            t.text_faint,
            Rgba::TRANSPARENT,
        );
        return;
    }
    let hottest = terms[0].heat.max(0.001);

    // Occupancy: terms are text, and overlapping text is unreadable. A
    // term that cannot find room this frame simply is not drawn — the
    // field thins out rather than smearing.
    let mut rows: Vec<Vec<(i32, i32)>> = vec![Vec::new(); rect.h.max(1) as usize];
    let cx = rect.x as f32 + rect.w as f32 * 0.5;
    let cy = rect.y as f32 + rect.h as f32 * 0.5;
    let sway = (frame % 240) as f32 / 240.0 * std::f32::consts::TAU;
    let breath = snap.state.motion() * 0.6;

    for term in terms.iter().take(DRAWN) {
        let heat = (term.heat / hottest).clamp(0.0, 1.0);
        // Hot terms pull toward the middle; cold ones drift out.
        let pull = 0.35 + 0.65 * (1.0 - heat);
        let wobble = breath * (sway + term.pos.0 * 6.0).sin() * 0.5;
        let x = cx + term.pos.0 * pull * (rect.w as f32 * 0.46) + wobble;
        let y = cy + term.pos.1 * pull * (rect.h as f32 * 0.44);
        let w = term.word.chars().count() as i32;
        let (xi, yi) = (x.round() as i32 - w / 2, y.round() as i32);
        if yi < rect.y || yi >= rect.y + rect.h || xi < rect.x || xi + w > rect.x + rect.w {
            continue;
        }
        let row = &mut rows[(yi - rect.y) as usize];
        if row.iter().any(|(a, b)| xi < *b + 1 && *a < xi + w + 1) {
            continue; // taken: keep the field readable
        }
        row.push((xi, xi + w));
        // The axis: your brief in the house red, the agent's trail in
        // blue. Brightness is heat.
        let hue = abstracttui::boot::identity::brand_ramp(term.origin.clamp(0.0, 1.0));
        let ink = mix(mix(t.bg, t.text_faint, 0.7), hue, 0.35 + 0.65 * heat);
        // A terminal has one font size, so "bigger" has to be spelled
        // some other way: the hottest terms shout in capitals.
        let shown = if heat > 0.72 {
            term.word.to_ascii_uppercase()
        } else {
            term.word.clone()
        };
        canvas.print(Point::new(xi, yi), &shown, ink, Rgba::TRANSPARENT);
    }

    // The attention cursor: it eases toward the centre of mass of the
    // hottest terms, so where it sits IS where the work is. It stops
    // moving when the run stops — the honesty rule, again.
    if snap.state.motion() > 0.0 {
        let hot: Vec<&&Term> = terms.iter().take(4).collect();
        let mass: f32 = hot.iter().map(|t| t.heat).sum::<f32>().max(0.001);
        let hx: f32 = hot.iter().map(|t| t.pos.0 * t.heat).sum::<f32>() / mass;
        let hy: f32 = hot.iter().map(|t| t.pos.1 * t.heat).sum::<f32>() / mass;
        let orbit = (frame % 40) as f32 / 40.0 * std::f32::consts::TAU;
        let x = cx + hx * 0.35 * rect.w as f32 * 0.46 + orbit.cos() * 2.0;
        let y = cy + hy * 0.35 * rect.h as f32 * 0.44 + orbit.sin();
        canvas.put(
            Point::new(x.round() as i32, y.round() as i32),
            '◦',
            snap.state.ink(t),
            Rgba::TRANSPARENT,
        );
    }

    // The legend is not optional: a colour axis nobody can read is a
    // decoration. Two words, bottom left, in the colours they name.
    let y = rect.y + rect.h - 1;
    canvas.print(
        Point::new(rect.x + 1, y),
        "your brief",
        abstracttui::boot::identity::brand_ramp(0.0),
        Rgba::TRANSPARENT,
    );
    canvas.print(
        Point::new(rect.x + 12, y),
        "·",
        t.text_faint,
        Rgba::TRANSPARENT,
    );
    canvas.print(
        Point::new(rect.x + 14, y),
        "its own trail",
        abstracttui::boot::identity::brand_ramp(1.0),
        Rgba::TRANSPARENT,
    );
    // How far the field has drifted, as a number, because the colour
    // alone must never be the only carrier (and 8% of readers cannot
    // separate these two hues at all).
    let drifted = feed.terms.iter().map(|t| t.heat * t.origin).sum::<f32>()
        / feed.terms.iter().map(|t| t.heat).sum::<f32>().max(0.001);
    let note = format!(
        "{:.0}% of the heat is the agent's own trail",
        drifted * 100.0
    );
    let note = abstracttui::text::truncate_ellipsis(&note, (rect.w - 32).max(4));
    canvas.print(
        Point::new(rect.x + rect.w - note.chars().count() as i32 - 1, y),
        &note,
        t.text_muted,
        Rgba::TRANSPARENT,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Positions are stable across calls and across processes — the
    /// field must not reshuffle when the pane is reopened.
    #[test]
    fn a_word_always_lands_in_the_same_place() {
        let a = disc_position("mosaic");
        let b = disc_position("mosaic");
        assert_eq!(a, b);
        assert_ne!(a, disc_position("mosaics"));
        // Inside the unit disc, always.
        for w in ["a", "mosaic", "src", "very_long_identifier_name"] {
            let (x, y) = disc_position(w);
            assert!(x * x + y * y <= 1.001, "{w} escaped the disc");
        }
    }

    /// Glue words, digits and runts never enter the field.
    #[test]
    fn the_field_refuses_noise() {
        let mut terms = Vec::new();
        for junk in ["the", "and", "12345", "a", "fn", "error"] {
            absorb_term(&mut terms, junk, 1.0, 0.0);
        }
        assert!(terms.is_empty(), "noise got in: {terms:?}");
        absorb_term(&mut terms, "mosaic", 1.0, 0.0);
        assert_eq!(terms.len(), 1);
    }

    /// Heat decays and cold terms leave, so the field follows the work
    /// instead of accumulating everything the run ever said.
    #[test]
    fn heat_decays_and_the_cold_drop_out() {
        let mut terms = Vec::new();
        absorb_term(&mut terms, "parser", 1.0, 0.0);
        for _ in 0..400 {
            decay(&mut terms, 0.985);
        }
        assert!(terms.is_empty(), "a term nobody mentions again cools away");
    }

    /// The origin axis moves toward what the sightings say.
    #[test]
    fn origin_mixes_toward_the_newest_source() {
        let mut terms = Vec::new();
        absorb_term(&mut terms, "mosaic", 1.0, 0.0); // the brief
        for _ in 0..20 {
            absorb_term(&mut terms, "mosaic", 1.0, 1.0); // the agent's trail
        }
        assert!(terms[0].origin > 0.5, "the trail pulled it over");
    }
}
