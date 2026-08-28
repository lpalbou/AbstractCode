//! animation — preview the `/animation` variants without a gateway.
//!
//! `cargo run --example animation -- --sheet <dir>` fabricates a run
//! (cycles, tool families, a failing streak) and renders every variant
//! at several pane sizes and run states to SVG plus an index.html.
//!
//! The fabricated feed is DEMO DATA and lives here, in an example — the
//! app never invents events.

use abstractcode_tui::store::Phase;
use abstractcode_tui::ui::animation::{
    desk, drift, pulse, Ev, Family, Feed, Outcome, Snapshot, State,
};
use abstracttui::base::{Rect, Size};
use abstracttui::render::{Cell, Screenshot, Surface};
use abstracttui::theme;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = args
        .iter()
        .position(|a| a == "--sheet")
        .and_then(|i| args.get(i + 1).cloned())
        .unwrap_or_else(|| "target/animation".into());
    let theme_id =
        std::env::var("ABSTRACTTUI_THEME").unwrap_or_else(|_| theme::DEFAULT_THEME_ID.into());
    let (theme, _) = theme::resolve(&theme_id);
    std::fs::create_dir_all(&dir).expect("sheet dir");

    let mut html = String::from(
        "<!doctype html><meta charset=utf-8><title>/animation</title>\
         <style>body{background:#0b0b0b;color:#ccc;font:12px system-ui;margin:14px}\
         h2{font-weight:600;margin:18px 0 6px}.g{display:grid;\
         grid-template-columns:repeat(2,1fr);gap:8px}figure{margin:0}\
         figcaption{opacity:.6;font-size:11px}\
         .shot{border:1px solid #2a2a2a}.shot svg{width:100%;height:auto;display:block}</style>",
    );

    let feed = demo_feed(45 * 60 * 1000);
    let sizes = [("120x36", Size::new(120, 36)), ("80x24", Size::new(80, 24))];
    let states = [
        ("working", State::Working),
        ("quiet", State::Quiet),
        ("waiting", State::Waiting),
        ("failing", State::Failing),
        ("down", State::Down),
    ];

    for (vi, variant) in ["pulse", "desk", "drift"].iter().enumerate() {
        html.push_str(&format!(
            "<h2>/animation {} · {variant}</h2><div class=g>",
            vi + 1
        ));
        for (label, size) in sizes {
            for (sname, state) in states {
                // Only the big pane gets every state; the small pane
                // shows the two that matter most.
                if size.w < 100 && !matches!(state, State::Working | State::Failing) {
                    continue;
                }
                let snap = demo_snapshot(state);
                let mut surface = Surface::new(size, Cell::EMPTY);
                let rect = Rect::new(0, 0, size.w, size.h);
                surface.fill_rect(
                    rect,
                    Cell::new(abstracttui::render::Glyph::SPACE)
                        .with_fg(theme.tokens.bg)
                        .with_bg(theme.tokens.bg),
                );
                match vi {
                    1 => desk::render(&mut surface, rect, &theme.tokens, &feed, &snap, 7),
                    2 => drift::render(&mut surface, rect, &theme.tokens, &feed, &snap, 7),
                    _ => pulse::render(&mut surface, rect, &theme.tokens, &feed, &snap, 7),
                }
                let svg = Screenshot::from_surface(&surface).to_svg();
                let name = format!("{variant}-{label}-{sname}.svg");
                let _ = std::fs::write(std::path::Path::new(&dir).join(&name), &svg);
                html.push_str(&format!(
                    "<figure><div class=shot>{svg}</div><figcaption>{label} · {sname}</figcaption></figure>"
                ));
            }
        }
        html.push_str("</div>");
    }
    let index = std::path::Path::new(&dir).join("index.html");
    let _ = std::fs::write(&index, html);
    println!("sheet: {}", index.display());
}

/// A plausible 45-minute run: 30 model cycles, ~2 tools each, a failing
/// streak at minute 18, and a vocabulary drawn from a believable brief.
fn demo_feed(span_ms: u64) -> Feed {
    let mut feed = Feed::new();
    let mut terms = Vec::new();
    for w in "make the mosaic dither quantize cleanly on 256 colour terminals".split_whitespace() {
        abstractcode_tui::ui::animation::drift::absorb_term(&mut terms, w, 3.0, 0.0);
    }
    let files = [
        "mosaic.rs",
        "dither.rs",
        "quantize.rs",
        "palette.rs",
        "mosaic_tests.rs",
    ];
    let mut evs = Vec::new();
    for i in 0..30u64 {
        let at = span_ms * i / 30;
        evs.push(Ev {
            at_ms: at,
            family: Family::Think,
            outcome: Outcome::Ok,
            tokens: 120 + (i * 137) % 900,
            label: String::new(),
        });
        for k in 0..2u64 {
            let n = i * 2 + k;
            let family = match n % 5 {
                0 => Family::Read,
                1 => Family::Search,
                2 => Family::Write,
                3 => Family::Exec,
                _ => Family::Net,
            };
            let failing = (36..44).contains(&n);
            let label = files[(n % files.len() as u64) as usize].to_string();
            abstractcode_tui::ui::animation::drift::absorb_term(&mut terms, &label, 1.0, 1.0);
            abstractcode_tui::ui::animation::drift::absorb_term(
                &mut terms,
                family.short(),
                0.6,
                1.0,
            );
            evs.push(Ev {
                at_ms: at + 400 * (k + 1),
                family,
                outcome: if failing {
                    Outcome::Failed
                } else {
                    Outcome::Ok
                },
                tokens: 0,
                label,
            });
        }
    }
    feed.events = evs.into();
    feed.terms = terms;
    feed
}

fn demo_snapshot(state: State) -> Snapshot {
    Snapshot {
        state,
        phase: if state == State::Idle {
            Phase::Idle
        } else {
            Phase::Running
        },
        elapsed_secs: 45 * 60 + 12,
        since_event_ms: match state {
            State::Quiet => 4 * 60 * 1000 + 12_000,
            _ => 2_400,
        },
        tok_per_s: Some(38.0),
        ctx_frac: Some(0.62),
        llm_calls: 30,
        tool_calls: 60,
        tool_failures: 8,
        activity: "editing js/game.js".into(),
    }
}
