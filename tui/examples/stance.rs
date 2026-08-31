//! stance — preview the `/stance` reads without a gateway.
//!
//! `cargo run --example stance -- --sheet <dir>` renders the one-line
//! form and the figure across a spread of turns (first turn with no
//! baseline, a light turn, a heavy turn, a failing turn, a turn with no
//! tools at all) to SVG plus an index.html.

use abstractcode::ui::stance::conduct::{self, Baseline, Facts, ToolCall};
use abstractcode::ui::stance::{self, Turn};
use abstracttui::base::{Rect, Size};
use abstracttui::render::{Cell, Glyph, Screenshot, Surface};
use abstracttui::theme;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = args
        .iter()
        .position(|a| a == "--sheet")
        .and_then(|i| args.get(i + 1).cloned())
        .unwrap_or_else(|| "target/stance".into());
    let theme_id =
        std::env::var("ABSTRACTTUI_THEME").unwrap_or_else(|_| theme::DEFAULT_THEME_ID.into());
    let (theme, _) = theme::resolve(&theme_id);
    let _ = std::fs::create_dir_all(&dir);
    let t = &theme.tokens;

    // A session's history, so the medians are real.
    let history: Vec<Turn> = (0..8)
        .map(|i| {
            turn(
                3200.0 + i as f64 * 400.0,
                300 + i * 40,
                2,
                &["read_file", "edit_file"],
            )
        })
        .collect();
    let base = stance::baseline(&history);

    let cases: Vec<(&str, Turn, Baseline)> = vec![
        (
            "first turn — no baseline yet",
            turn(4200.0, 380, 2, &["read_file", "edit_file"]),
            Baseline::default(),
        ),
        ("a light turn", turn(1800.0, 140, 1, &["read_file"]), base),
        (
            "a heavy turn",
            turn(
                38_000.0,
                2400,
                6,
                &[
                    "read_file",
                    "search_files",
                    "edit_file",
                    "execute_command",
                    "read_file",
                    "edit_file",
                    "web_search",
                ],
            ),
            base,
        ),
        ("a failing turn", failing(), base),
        (
            "thinking only — no calls to read",
            turn(9000.0, 700, 0, &[]),
            base,
        ),
        (
            "an entity visit — the only lane with a recall fact",
            {
                let mut t = turn(6000.0, 520, 2, &["read_file", "web_search"]);
                t.facts.memories_recalled = Some(7);
                t.facts.memories_formed = Some(2);
                t
            },
            base,
        ),
    ];

    let mut html = String::from(
        "<!doctype html><meta charset=utf-8><title>/stance</title>\
         <style>body{background:#0b0b0b;color:#ccc;font:12px system-ui;margin:14px}\
         h3{margin:16px 0 4px;font-weight:600}figcaption{opacity:.6;font-size:11px}\
         .shot{border:1px solid #2a2a2a;margin-bottom:8px}\
         .shot svg{width:100%;height:auto;display:block}</style>",
    );
    for (label, turn, base) in cases {
        let mut axes = conduct::axes(&turn.facts, &turn.tools, &base);
        // Mirror the app: the agent lane cannot make a recall read, so
        // it is not shown there. The last case below is the entity lane,
        // which has the fact and keeps all four.
        if turn.facts.memories_recalled.is_none() {
            axes.retain(|a| a.id != conduct::AxisId::Attention);
        }
        html.push_str(&format!("<h3>{label}</h3>"));
        for (name, size, figure) in [
            ("line", Size::new(96, 1), false),
            ("figure", Size::new(96, 14), true),
            // The size the floating panel actually gives it (card 38x12
            // minus its border): what ships is what is previewed.
            ("card", Size::new(36, 10), true),
        ] {
            let mut s = Surface::new(size, Cell::EMPTY);
            let rect = Rect::new(0, 0, size.w, size.h);
            s.fill_rect(rect, Cell::new(Glyph::SPACE).with_fg(t.bg).with_bg(t.bg));
            if figure {
                stance::draw_figure(&mut s, rect, t, &axes, &turn, 6, true);
            } else {
                stance::draw_line(&mut s, rect, t, &axes);
            }
            let svg = Screenshot::from_surface(&s).to_svg();
            let file = format!(
                "{}-{name}.svg",
                label.replace(|c: char| !c.is_alphanumeric(), "-")
            );
            let _ = std::fs::write(std::path::Path::new(&dir).join(&file), &svg);
            html.push_str(&format!("<div class=shot>{svg}</div>"));
        }
    }
    let index = std::path::Path::new(&dir).join("index.html");
    let _ = std::fs::write(&index, html);
    println!("sheet: {}", index.display());
}

fn turn(think_ms: f64, tokens_out: u64, rounds: u32, tools: &[&str]) -> Turn {
    Turn {
        facts: Facts {
            think_ms: Some(think_ms),
            tokens_out: Some(tokens_out),
            tool_rounds: Some(rounds),
            ..Facts::default()
        },
        tools: tools
            .iter()
            .map(|n| ToolCall {
                name: (*n).into(),
                ok: Some(true),
            })
            .collect(),
    }
}

fn failing() -> Turn {
    let mut t = turn(22_000.0, 900, 4, &[]);
    t.tools = [
        ("read_file", true),
        ("edit_file", false),
        ("edit_file", false),
        ("edit_file", false),
        ("execute_command", false),
    ]
    .iter()
    .map(|(n, ok)| ToolCall {
        name: (*n).into(),
        ok: Some(*ok),
    })
    .collect();
    t
}
